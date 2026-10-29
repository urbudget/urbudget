import {
  memo,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type MutableRefObject,
  type ReactNode,
} from "react";
import { AlertTriangle, Check, CircleMinus, CirclePlus, Layers, Pencil, RotateCcw, Rows3, Sparkles, Split, X } from "lucide-react";
import type { BankField, BatchInfo, Statement, StatementLine } from "../bridge";
import { editableAmount, errorText, evaluateAmount, flowAmountChange, formatDate, money } from "../format";
import type { BankColumns, Budget } from "../types";
import { PayeeInput, transferPayee, useVirtualRows } from "../ui";
import { textWidth, useColumns } from "../ui/columns";
import { FIELDS, INCLUDABLE, ROLES, leftOutText } from "./model";
import { formatCount, getDirection, t, tc, tn, tnc } from "../i18n";

/** A line is this tall, so a long file only renders the lines in view (as the register's rows). */
const LINE = 34;
/** The line being edited: its fields (40px under 4px of padding), its footer (48px) and its edge. */
const EDIT = 4 + 40 + 48 + 1;
/** The column of what can be done to each line: two icons, which open leftward over the line. */
const ACTIONS = 64;

/** What can be done to lines, the same functions for every render. */
export type LineOps = {
  /** Cells given new values, each from the value it was changed from, against the line as it reads when the change is made. */
  cells: (line: StatementLine, changes: { column: number; from: string; to: string }[], fields?: FieldChange[]) => void;
  include: (lines: StatementLine[], on: boolean) => void;
  revert: (lines: StatementLine[]) => void;
  header: (line: StatementLine) => void;
  whole: (entry: string, on: boolean) => void;
  role: (column: number, role: keyof BankColumns | null) => void;
};
/** Cells given new values, shown as them until the file reads again. */
type Pending = { line: string; values: Record<number, string>; fields: Partial<Record<BankField, string>>; revision: number };
/** A line's cells as the editor opened on them, and what its fields hold now. */
type FieldChange = { field: BankField; from: string; to: string };
type Draft = { opened: string[]; values: string[]; fields: FieldChange[] };

const includable = (l: StatementLine) => l.status.kind === "leftOut" && INCLUDABLE.has(l.status.reason) && !l.included;

/**
 * The file's lines as the register lists transactions: a click selects a line (Ctrl adds one,
 * Shift a run of them, as do the checkboxes), another click (or Enter) edits it, all its cells at
 * once in a row of fields, Enter saving and Esc leaving it. Selected lines are included, left out
 * or put back together from the bar that comes up under them. What can be done to one line sits
 * at its right end, quiet until the line is pointed at. The first column says how each line
 * imports, or why it doesn't.
 */
export function LineTable({
  budget,
  statement,
  lines: tab,
  role,
  busy,
  slow,
  assisted,
  ops,
  empty,
}: {
  budget: Budget;
  statement: Statement;
  /** The lines the tab shows. */
  lines: StatementLine[];
  /** What each column holds, by its place. */
  role: readonly (keyof BankColumns | null)[];
  /** A change is being applied: the cells given meanwhile show until the file reads again. */
  busy: boolean;
  /** One that lasts: the lines say so. */
  slow: boolean;
  assisted: ReadonlySet<string>;
  ops: LineOps;
  /** Shown when the tab has no lines. */
  empty: ReactNode;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  const [cursor, setCursor] = useState<string | null>(null);
  // The keys moved the cursor: its line is marked until the pointer is used again.
  const [keyed, setKeyed] = useState(false);
  const anchor = useRef<string | null>(null);
  const [editing, setEditing] = useState<{ line: string; column: number } | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const table = statement.format === "csv";
  const headers = statement.headers;
  const width = Math.max(1, headers.length);
  const place = useMemo(() => new Map(statement.lines.map((l, i) => [l.id, i])), [statement.lines]);
  // The line being edited stays, in its place, while it's edited, though it has left the tab
  // meanwhile (another change made it read): what's typed in it isn't lost.
  const lines = useMemo(() => {
    const id = editing?.line;
    if (id === undefined || tab.some((l) => l.id === id)) return tab;
    const at = place.get(id);
    if (at === undefined) return tab;
    const after = tab.findIndex((l) => (place.get(l.id) ?? 0) > at);
    return after < 0 ? [...tab, statement.lines[at]] : [...tab.slice(0, after), statement.lines[at], ...tab.slice(after)];
  }, [tab, editing?.line, place, statement.lines]);
  // A new reading of the file shows the cells as they read; a change that didn't apply, as they were.
  useEffect(() => setPending((p) => (p && p.revision !== statement.revision ? null : p)), [statement.revision]);
  useEffect(() => {
    if (!busy) setPending((p) => (p && p.revision === statement.revision ? null : p));
  }, [busy]);
  // A line no longer in the file isn't being edited.
  useEffect(() => {
    if (editing && !lines.some((l) => l.id === editing.line)) setEditing(null);
  }, [lines]);
  // The selection is of the lines the tab shows, in their order.
  const chosen = useMemo(() => (selected.size ? lines.filter((l) => selected.has(l.id)) : []), [lines, selected]);

  // Columns as wide as what they hold (each file's own), resizable as the register's.
  const fitted = useMemo(() => {
    const sample = statement.lines.slice(0, 300);
    return headers.map((h, i) => {
      let longest = "";
      for (const l of sample) if ((l.cells[i]?.length ?? 0) > longest.length) longest = l.cells[i];
      const head = textWidth((h || columnLabel(i)).toUpperCase(), 11, 600) + (table ? 74 : 0);
      return Math.min(320, Math.max(84, Math.max(head, textWidth(longest, 13)) + 24));
    });
  }, [headers, statement.lines, table]);
  // Widths someone set are kept for files laid out the same way.
  const columns = useColumns(
    `import-lines:${layoutKey(headers)}`,
    Object.fromEntries([["state", 236], ...fitted.map((w, i) => [`c${i}`, w])]) as Record<string, number>,
    { state: 170 },
  );
  const tracks = [`${columns.widths.state}px`, ...fitted.map((_, i) => `${columns.widths[`c${i}`]}px`)];
  // The last column takes any room to spare, so the table fills the page.
  tracks[tracks.length - 1] = `minmax(${tracks[tracks.length - 1]}, 1fr)`;
  const style = {
    "--line-cols": [...tracks, `${ACTIONS}px`].join(" "),
    minWidth: columns.widths.state + fitted.reduce((n, _, i) => n + columns.widths[`c${i}`], 0) + ACTIONS,
  } as CSSProperties;

  // Only the lines in view render; the line being edited is taller.
  const view = useVirtualRows(scroller, lines.length, LINE);
  const editAt = editing ? lines.findIndex((l) => l.id === editing.line) : -1;
  const extra = EDIT - LINE;
  const before = view.before + (editAt >= 0 && editAt < view.start ? extra : 0);
  const after = view.after + (editAt >= view.end ? extra : 0);

  // The handlers read the table as it is now, so the rows keep the same ones.
  const latest = useRef({ lines, selected, cursor, table, commit: (_keep?: string): boolean => true });
  const act = useMemo(() => {
    const ids = (a: string, b: string) => {
      const all = latest.current.lines;
      const i = all.findIndex((l) => l.id === a);
      const j = all.findIndex((l) => l.id === b);
      if (i < 0 || j < 0) return [b];
      return all.slice(Math.min(i, j), Math.max(i, j) + 1).map((l) => l.id);
    };
    // A line can be edited while a change applies: what it saves waits for that change.
    const edit = (line: StatementLine, column?: number) => {
      if (!latest.current.commit(line.id)) return;
      setCursor(line.id);
      setSelected(new Set([line.id]));
      anchor.current = line.id;
      // Where the line doesn't read, else where it was clicked, else its first cell.
      const bad = line.status.kind === "attention" && latest.current.table ? line.status.column : undefined;
      setEditing({ line: line.id, column: column ?? bad ?? 0 });
    };
    /** A click on a line: it alone, or with Ctrl added or taken away, or with Shift a run of lines. */
    const pick = (line: StatementLine, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => {
      const add = e.ctrlKey || e.metaKey;
      if (e.shiftKey && anchor.current) {
        const run = ids(anchor.current, line.id);
        setSelected((s) => new Set(add ? [...s, ...run] : run));
      } else {
        setSelected((s) => {
          if (!add) return new Set([line.id]);
          const next = new Set(s);
          if (next.has(line.id)) next.delete(line.id);
          else next.add(line.id);
          return next;
        });
        anchor.current = line.id;
      }
      setCursor(line.id);
    };
    /** A line's checkbox (or beside it): added or taken away, the rest kept; with Shift, the run to it. */
    const check = (line: StatementLine, shift: boolean) => {
      const on = !latest.current.selected.has(line.id);
      const run = shift && anchor.current ? ids(anchor.current, line.id) : [line.id];
      setSelected((s) => {
        const next = new Set(s);
        for (const id of run) {
          if (on) next.add(id);
          else next.delete(id);
        }
        return next;
      });
      anchor.current = line.id;
      setCursor(line.id);
    };
    const click = (line: StatementLine) => (e: MouseEvent<HTMLDivElement>) => {
      const target = e.target as HTMLElement;
      if (target.closest("button, input, select, a")) return;
      if (target.closest(".line-check")) return check(line, e.shiftKey);
      const column = Number(target.closest<HTMLElement>("[data-column]")?.dataset.column ?? NaN);
      const now = latest.current;
      // A click away from the line being edited saves it, as a spreadsheet's does.
      if (!now.commit(line.id)) return;
      // A second click on the only selected line edits it, starting in the clicked cell.
      const plain = !e.shiftKey && !e.ctrlKey && !e.metaKey;
      if (plain && now.cursor === line.id && now.selected.size === 1 && now.selected.has(line.id))
        edit(line, Number.isNaN(column) ? undefined : column);
      else pick(line, e);
    };
    const clear = () => {
      setSelected(new Set());
      anchor.current = null;
    };
    return { edit, pick, check, click, clear };
  }, []);
  const focusGrid = () => requestAnimationFrame(() => scroller.current?.focus({ preventScroll: true }));
  const close = () => {
    setEditing(null);
    focusGrid();
  };
  // Only the cells typed over are saved, each from what it held when the editor opened: a change
  // that landed on the line meanwhile (the assistant's, an earlier edit still being read) stays.
  const save = (line: StatementLine, { opened, values, fields }: Draft) => {
    const changes = values
      .map((to, column) => ({ column, from: opened[column] ?? "", to: to.trim() }))
      .filter((c) => c.to !== c.from.trim());
    close();
    if (!changes.length && !fields.length) return;
    const given = Object.fromEntries(changes.map((c) => [c.column, c.to]));
    setPending((p) => ({
      line: line.id,
      // An earlier edit of the line still on its way keeps showing too.
      values: p?.line === line.id && p.revision === statement.revision ? { ...p.values, ...given } : given,
      fields: { ...(p?.line === line.id && p.revision === statement.revision ? p.fields : {}), ...Object.fromEntries(fields.map((f) => [f.field, f.to])) },
      revision: statement.revision,
    }));
    ops.cells(line, changes, fields);
  };
  // What the line being edited holds now, saved when the selection moves away from it.
  const draft = useRef<(() => boolean) | null>(null);
  const commit = (keep?: string) => {
    if (!editing || editing.line === keep) return true;
    return draft.current?.() ?? true;
  };
  latest.current = { lines, selected, cursor, table, commit };
  // Cells that scroll sideways pass under the pinned columns: a shadow says so, only then.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // Under the start (the left; right to left, the right) and under the end. Right to left,
    // scrollLeft runs from 0 at the start towards negative values.
    const edges = () => {
      const scrolled = Math.abs(el.scrollLeft);
      el.classList.toggle("under-left", scrolled > 0);
      el.classList.toggle("under-right", scrolled + el.clientWidth < el.scrollWidth - 1);
    };
    edges();
    el.addEventListener("scroll", edges, { passive: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(edges);
    observer?.observe(el);
    if (el.firstElementChild) observer?.observe(el.firstElementChild);
    return () => {
      el.removeEventListener("scroll", edges);
      observer?.disconnect();
    };
  }, []);
  const reveal = (index: number) => {
    const el = scroller.current;
    if (!el) return;
    // Under the sticky heading.
    const head = el.querySelector<HTMLElement>(".line-head")?.offsetHeight ?? LINE;
    const top = index * LINE + head;
    if (top - head < el.scrollTop) el.scrollTop = top - head;
    else if (top + LINE > el.scrollTop + el.clientHeight) el.scrollTop = top + LINE - el.clientHeight;
  };
  // As the register's keys: the arrows move the selection (with Shift, stretch it), Space adds the
  // line or takes it away, Enter edits it, Ctrl+A selects every line shown, Esc lets them go.
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    // Keys belong to the field being typed in; a line's checkbox lets them through.
    const checkbox = target instanceof HTMLInputElement && target.type === "checkbox";
    if (target.closest("button, select, .line-editor") || (target.closest("input") && !checkbox)) return;
    const index = cursor ? lines.findIndex((l) => l.id === cursor) : -1;
    const move = (to: number) => {
      const next = lines[Math.min(lines.length - 1, Math.max(0, to))];
      if (!next) return;
      e.preventDefault();
      if (!commit()) return;
      if (e.shiftKey) {
        anchor.current ??= cursor ?? next.id;
        const i = lines.findIndex((l) => l.id === anchor.current);
        const j = lines.indexOf(next);
        setSelected(new Set(lines.slice(Math.min(i, j), Math.max(i, j) + 1).map((l) => l.id)));
      } else {
        setSelected(new Set([next.id]));
        anchor.current = next.id;
      }
      setCursor(next.id);
      setKeyed(true);
      reveal(lines.indexOf(next));
    };
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / LINE) - 2);
    if (e.key === "ArrowDown") move(index + 1);
    else if (e.key === "ArrowUp") move(index < 0 ? 0 : index - 1);
    else if (e.key === "PageDown") move(index + page);
    else if (e.key === "PageUp") move(index - page);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(lines.length - 1);
    else if ((e.key === "Enter" || e.key === "F2") && index >= 0) {
      e.preventDefault();
      act.edit(lines[index]);
    } else if (e.key === " " && index >= 0 && !checkbox) {
      e.preventDefault();
      setKeyed(true);
      act.check(lines[index], false);
    } else if (e.key === "Escape" && editing) {
      e.preventDefault();
      close();
    } else if (e.key === "Escape" && selected.size) {
      e.preventDefault();
      act.clear();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelected(new Set(lines.map((l) => l.id)));
    }
  };
  // Esc lets the selection go from anywhere on the page, unless it's a field's, or a menu's.
  useEffect(() => {
    if (!selected.size) return;
    const escape = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const target = e.target as HTMLElement;
      const typing =
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLInputElement && target.type !== "checkbox");
      if (typing || target.closest?.(".line-editor") || document.querySelector(".modal-shade, .popover")) return;
      act.clear();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [selected.size > 0]);
  const all = lines.length > 0 && chosen.length === lines.length;
  return (
    <>
      <div
        className={`line-scroll${slow ? " slow" : ""}${keyed ? " keyed" : ""}${chosen.length ? " has-selection" : ""}`}
        ref={scroller}
        tabIndex={0}
        onKeyDown={keys}
        onPointerDown={() => setKeyed(false)}
      >
        <div
          className="line-table"
          role="grid"
          aria-label={t("The file's lines")}
          aria-rowcount={lines.length + 1}
          aria-multiselectable
          style={style}
        >
          <div className="line-row line-head" role="row">
            <span role="columnheader" className="line-state" title={t("How each line imports, or why it doesn't")}>
              <span className="line-check">
                <input
                  type="checkbox"
                  aria-label={t("Select every line shown")}
                  checked={all}
                  disabled={!lines.length}
                  ref={(el) => {
                    if (el) el.indeterminate = chosen.length > 0 && !all;
                  }}
                  onChange={() => (all ? act.clear() : setSelected(new Set(lines.map((l) => l.id))))}
                />
              </span>
              {t("Imports as")}
              {columns.grip("state", t("Imports as"))}
            </span>
            {headers.map((h, i) => (
              <span role="columnheader" key={i} className="line-col">
                <span className="line-col-name" title={h || columnLabel(i)}>
                  {h || columnLabel(i)}
                </span>
                {table && <RolePick column={i} role={role[i] ?? null} busy={slow} onChange={(r) => ops.role(i, r)} />}
                {columns.grip(`c${i}`, h || columnLabel(i))}
              </span>
            ))}
            <span role="columnheader" className="line-actions-cell" aria-label={t("What can be done")} />
          </div>
          {before > 0 && <div aria-hidden style={{ height: before }} />}
          {lines
            .slice(view.start, view.end)
            .map((line) =>
              editing?.line === line.id ? (
                <LineEditor
                  key={line.id}
                  budget={budget}
                  account={statement.scope.account}
                  line={line}
                  headers={headers}
                  role={role}
                  csv={table}
                  locked={table ? role.map((r) => r === "id") : []}
                  focus={editing.column}
                  waiting={pending?.line === line.id ? pending.values : null}
                  waitingFields={pending?.line === line.id ? pending.fields : undefined}
                  draft={draft}
                  onSave={(d) => save(line, d)}
                  onCancel={close}
                />
              ) : (
                <LineRow
                  key={line.id}
                  line={line}
                  index={place.get(line.id) ?? -1}
                  batch={line.batch !== undefined ? statement.batches?.[line.batch] : undefined}
                  headers={headers}
                  role={role}
                  transferName={budget.accounts.find((a) => a.id === line.fields.transferAccountId)?.name ?? line.fields.transferAccountId}
                  table={table}
                  width={width}
                  busy={slow}
                  selected={selected.has(line.id)}
                  cursor={cursor === line.id}
                  assisted={assisted.has(line.id)}
                  pending={pending?.line === line.id ? pending.values : null}
                  ops={ops}
                  onClick={act.click}
                  onCheck={act.check}
                  onEdit={act.edit}
                />
              ),
            )}
          {after > 0 && <div aria-hidden style={{ height: after }} />}
        </div>
        {!lines.length && <div className="line-empty">{empty}</div>}
      </div>
      {chosen.length > 0 && !editing && <LineBulkBar chosen={chosen} busy={slow} ops={ops} onClear={act.clear} />}
    </>
  );
}

/** A column the file gives no name. */
const columnLabel = (i: number) => t("Column {number}", { number: i + 1 });

/** A short name for a file's layout: its columns' names. */
function layoutKey(headers: string[]) {
  let hash = 0;
  for (const c of headers.join("")) hash = (Math.imul(hash, 31) + c.charCodeAt(0)) | 0;
  return `${headers.length}-${(hash >>> 0).toString(36)}`;
}

/**
 * What can be done to the selected lines, floating over the bottom of the table as the register's
 * bar does: how many (the pill, which also lets them go), then each change that fits some of them.
 */
function LineBulkBar({ chosen, busy, ops, onClear }: { chosen: StatementLine[]; busy: boolean; ops: LineOps; onClear: () => void }) {
  const toInclude = chosen.filter(includable);
  const toLeave = chosen.filter((l) => l.included);
  const toRevert = chosen.filter((l) => l.fixed);
  const count = (k: number) => formatCount(k);
  const actions = [
    {
      name: t("Include"),
      lines: toInclude,
      icon: <CirclePlus size={15} />,
      title: tn(toInclude.length, "Import 1 line though left out", "Import {count} lines though left out", { count: count(toInclude.length) }),
      run: () => ops.include(toInclude, true),
    },
    {
      name: t("Leave out"),
      lines: toLeave,
      icon: <CircleMinus size={15} />,
      title: tn(toLeave.length, "Leave 1 line out again", "Leave {count} lines out again", { count: count(toLeave.length) }),
      run: () => ops.include(toLeave, false),
    },
    {
      name: t("Put back"),
      lines: toRevert,
      icon: <RotateCcw size={14} />,
      title: tn(toRevert.length, "Put 1 line back as the file has it", "Put {count} lines back as the file has them", {
        count: count(toRevert.length),
      }),
      run: () => ops.revert(toRevert),
    },
  ].filter((a) => a.lines.length);
  return (
    <div className="bulk-dock line-bulk">
      <div className="bulk-bar" role="toolbar" aria-label={t("Selected lines")}>
        <span className="bulk-count">
          <span aria-live="polite">{tnc("lines", chosen.length, "{count} selected", "{count} selected", { count: formatCount(chosen.length) })}</span>
          <button aria-label={t("Clear selection")} title={t("Clear selection (Esc)")} onClick={onClear}>
            <X size={14} />
          </button>
        </span>
        {actions.length ? (
          <span className="bulk-actions">
            {actions.map((a) => (
              <button key={a.name} className="bulk-action named" disabled={busy} title={a.title} onClick={a.run}>
                {a.icon}
                <span>
                  {a.name}
                  {a.lines.length < chosen.length && <small> {formatCount(a.lines.length)}</small>}
                </span>
              </button>
            ))}
          </span>
        ) : (
          <span className="bulk-note">{t("Nothing to change on these together")}</span>
        )}
      </div>
    </div>
  );
}

/** What a column holds, chosen in its heading: a quiet chip over a menu. */
function RolePick({
  column,
  role,
  busy,
  onChange,
}: {
  column: number;
  role: keyof BankColumns | null;
  busy: boolean;
  onChange: (role: keyof BankColumns | null) => void;
}) {
  const label = ROLES.find(([r]) => r === role)?.[1];
  return (
    <label className={`role-pick${role ? " on" : ""}`} title={t("What this column holds")}>
      <span aria-hidden>{label ? t(label) : t("Not read")}</span>
      <select
        aria-label={t("What column {number} holds", { number: column + 1 })}
        value={role ?? ""}
        disabled={busy}
        onChange={(e) => onChange((e.target.value || null) as keyof BankColumns | null)}
      >
        <option value="">{t("Not read")}</option>
        {ROLES.map(([r, name]) => (
          <option key={r} value={r}>
            {t(name)}
          </option>
        ))}
      </select>
    </label>
  );
}

/** A line of the file. Redrawn only when what it shows changes. */
const LineRow = memo(function LineRow({
  line,
  index,
  batch,
  headers,
  role,
  transferName,
  table,
  width,
  busy,
  selected,
  cursor,
  assisted,
  pending,
  ops,
  onClick,
  onCheck,
  onEdit,
}: {
  line: StatementLine;
  /** Where it is among the file's lines, as a batch names its payments. */
  index: number;
  batch?: BatchInfo;
  headers: string[];
  role: readonly (keyof BankColumns | null)[];
  transferName?: string;
  table: boolean;
  width: number;
  busy: boolean;
  selected: boolean;
  cursor: boolean;
  assisted: boolean;
  pending: Record<number, string> | null;
  ops: LineOps;
  onClick: (line: StatementLine) => (e: MouseEvent<HTMLDivElement>) => void;
  onCheck: (line: StatementLine, shift: boolean) => void;
  onEdit: (line: StatementLine, column?: number) => void;
}) {
  const s = line.status;
  const changed = !!(line.fixed || line.included);
  return (
    <div
      role="row"
      aria-selected={selected}
      data-line={line.id}
      className={`line-row ${s.kind}${changed ? " changed" : ""}${selected ? " selected" : ""}${cursor ? " cursor" : ""}`}
      onClick={onClick(line)}
      onDoubleClick={(e) => {
        const target = e.target as HTMLElement;
        if (target.closest("button, input, select, .line-check")) return;
        const column = Number(target.closest<HTMLElement>("[data-column]")?.dataset.column ?? NaN);
        onEdit(line, Number.isNaN(column) ? undefined : column);
      }}
    >
      <span role="cell" className="line-state">
        <span className="line-check">
          <input
            type="checkbox"
            aria-label={t("Select line {line}", { line: line.id })}
            checked={selected}
            onChange={(e) => onCheck(line, (e.nativeEvent as globalThis.MouseEvent).shiftKey)}
          />
        </span>
        {s.kind === "ready" ? (
          <ReadsAs line={line} assisted={assisted} />
        ) : s.kind === "attention" ? (
          <span className="line-attention" title={s.reason}>
            <AlertTriangle size={14} aria-hidden />
            <span>{s.reason}</span>
          </span>
        ) : (
          <span className="line-left-out" title={leftOutText(line)}>
            {line.included ? `${t("Included")} · ` : ""}
            {leftOutText(line)}
          </span>
        )}
      </span>
      {Array.from({ length: width }, (_, column) => {
        const waiting = pending?.[column] ?? null;
        const value = role[column] === "payee" && transferName
          ? transferPayee(transferName)
          : role[column] === "transferAccountId" && transferName
            ? transferName
            : waiting ?? line.cells[column] ?? "";
        const bad = waiting === null && s.kind === "attention" && (table ? s.column === column : FIELDS[s.field] === headers[column]);
        const edited = waiting !== null || (!!line.original?.length && (line.original[column] ?? "") !== value);
        return (
          <span
            role="cell"
            key={column}
            data-column={column}
            className={`line-cell${bad ? " bad" : ""}${edited ? " edited" : ""}${waiting !== null ? " waiting" : ""}`}
            aria-label={t("{column} of line {line}: {value}", { column: headers[column] || columnLabel(column), line: line.id, value: value || t("empty") })}
            title={edited && line.original?.length ? t("In the file: {value}", { value: line.original[column] ?? "" }) : undefined}
          >
            {value}
          </span>
        );
      })}
      <span role="cell" className="line-actions-cell">
        <LineActions line={line} index={index} batch={batch} assisted={assisted} busy={busy} ops={ops} onEdit={onEdit} />
      </span>
    </div>
  );
});

/**
 * What can be done to a line, as the register's scheduled rows have it: quiet icons at the line's
 * right end, which open into a raised row of named buttons when the line is pointed at or tabbed to.
 */
function LineActions({
  line,
  index,
  batch,
  assisted,
  busy,
  ops,
  onEdit,
}: {
  line: StatementLine;
  index: number;
  batch?: BatchInfo;
  assisted: boolean;
  busy: boolean;
  ops: LineOps;
  onEdit: (line: StatementLine, column?: number) => void;
}) {
  const s = line.status;
  const id = line.id;
  // Each with a short label, and a name that says it all to whoever can't see the line.
  const list: { label: string; name: string; title: string; icon: ReactNode; run: () => void }[] = [];
  if (s.kind === "attention")
    list.push({ label: t("Fix"), name: t("Fix line {line}", { line: id }), title: s.reason, icon: <Pencil size={13} />, run: () => onEdit(line) });
  if (includable(line))
    list.push({
      label: t("Include"),
      name: t("Include line {line}", { line: id }),
      title: t("Import this line though it's left out"),
      icon: <CirclePlus size={14} />,
      run: () => ops.include([line], true),
    });
  if (line.included)
    list.push({
      label: t("Leave out"),
      name: t("Leave line {line} out", { line: id }),
      title: t("Leave this line out again"),
      icon: <CircleMinus size={14} />,
      run: () => ops.include([line], false),
    });
  if (s.kind === "leftOut" && s.reason === "aboveHeader")
    list.push({
      label: t("Header"),
      name: t("Use line {line} as the header row", { line: id }),
      title: t("Use this line as the header row: the columns take its names"),
      icon: <Rows3 size={14} />,
      run: () => ops.header(line),
    });
  if (batch && batch.addsUp === false && !batch.asOne && batch.payments[0] === index)
    list.push({
      label: t("As one"),
      name: t("Import as one entry"),
      title: t("Import the batch as the bank's one entry, not its payments"),
      icon: <Layers size={14} />,
      run: () => ops.whole(batch.entry, true),
    });
  if (batch?.asOne && batch.line === index)
    list.push({
      label: tc("batch", "Split"),
      name: t("Import the payments one by one"),
      title: t("Import the batch's payments one by one"),
      icon: <Split size={14} />,
      run: () => ops.whole(batch.entry, false),
    });
  if (line.fixed)
    list.push({
      label: t("Undo"),
      name: t("Put line {line} back as the file has it", { line: id }),
      title: assisted ? t("Fixed by the assistant · put back as the file has it") : t("Put back as the file has it"),
      icon: assisted ? <Sparkles size={13} /> : <RotateCcw size={13} />,
      run: () => ops.revert([line]),
    });
  if (!list.length) return null;
  return (
    <span className="line-actions">
      {list.map((a) => (
        <button
          key={a.label}
          type="button"
          className="line-action"
          disabled={busy}
          title={a.title}
          aria-label={a.name}
          onClick={(e) => {
            e.stopPropagation();
            a.run();
          }}
        >
          {a.icon}
          <span className="line-action-label">{a.label}</span>
        </button>
      ))}
    </span>
  );
}

/** A line that reads: its date and amount as they import, lit up briefly when a change alters them. */
function ReadsAs({ line, assisted }: { line: StatementLine; assisted: boolean }) {
  const cents = Math.round(Number(line.fields.amount) * 100);
  const changed = line.fixed || line.included;
  const why = changed ? (assisted ? t("Fixed by the assistant") : line.included ? t("Included") : t("Changed")) : "";
  const amount = Number.isFinite(cents) ? money(cents, { symbol: false }) : line.fields.amount;
  // A new reading of the same line (the signs turned, another date order) lights up as it changes,
  // counted as it renders rather than with a render of its own for every line.
  const reading = `${line.fields.date}|${line.fields.amount}`;
  const seen = useRef({ reading, moved: 0 });
  if (seen.current.reading !== reading) seen.current = { reading, moved: seen.current.moved + 1 };
  const moved = seen.current.moved;
  return (
    <span
      className={`line-ok${changed ? " line-changed" : ""}`}
      title={[why, t("Imports as {date}, {amount}", { date: formatDate(line.fields.date), amount })].filter(Boolean).join(" · ")}
    >
      <Check size={14} aria-label={t("Reads")} />
      <span key={moved} className={`line-reading${moved ? " moved" : ""}`}>
        <span className="line-date">{formatDate(line.fields.date)}</span>
        <span className={`line-amount${cents > 0 ? " in" : ""}`}>{amount}</span>
      </span>
    </span>
  );
}

/**
 * A line being edited, as the register edits a transaction: every cell a field, Tab moving
 * through them, Enter saving and Esc leaving it as it was. A bank's ID isn't changed.
 */
function LineEditor({
  budget,
  account,
  line,
  headers,
  role,
  csv,
  locked,
  focus,
  waiting,
  waitingFields,
  draft,
  onSave,
  onCancel,
}: {
  budget: Budget;
  account: string;
  line: StatementLine;
  headers: string[];
  role: readonly (keyof BankColumns | null)[];
  csv: boolean;
  /** Columns that can't be changed (the bank's ID). */
  locked: boolean[];
  /** The cell to start in. */
  focus: number;
  /** The line's cells given new values that the file doesn't read with yet. */
  waiting: Record<number, string> | null;
  waitingFields?: Partial<Record<BankField, string>>;
  /** Where what the fields hold is kept, for the table to save the line when the selection moves away. */
  draft: MutableRefObject<(() => boolean) | null>;
  onSave: (draft: Draft) => void;
  onCancel: () => void;
}) {
  // The cells as they show when it opens, kept: what's saved is what's typed over them.
  const [opened] = useState(() => headers.map((_, i) => waiting?.[i] ?? line.cells[i] ?? ""));
  const [openedFields] = useState(() => ({ ...line.fields, ...waitingFields }));
  // Money starts as the effective transaction amount, in the same format and direction as the
  // register. Raw bank signs, fees and accounting parentheses were already read by the engine.
  // Keep its source cells separately: merely opening or saving another field never changes them.
  const [initial] = useState(() => {
    const amount = Number(openedFields.amount);
    const parsed = line.status.kind === "ready" || csv && line.status.kind === "attention" && line.status.field === "date";
    if ((!waitingFields?.amount && !parsed) || !/^-?\d+(\.\d{1,2})?$/.test(openedFields.amount) || !Number.isFinite(amount)) return opened;
    const cents = Math.round(amount * 100);
    const at = role.indexOf("amount");
    const hasOut = role.includes("outflow"), hasIn = role.includes("inflow");
    const signed = at >= 0 && (!!opened[at].trim() || !hasOut && !hasIn);
    return opened.map((v, i) => {
      if (role[i] === "amount") return signed ? editableAmount(cents) : "";
      if (role[i] === "outflow") return signed || hasIn && cents >= 0 ? "" : editableAmount(-cents);
      if (role[i] === "inflow") return signed || hasOut && cents <= 0 ? "" : editableAmount(cents);
      return v;
    });
  });
  const [values, setValues] = useState(initial);
  const [transfer, setTransfer] = useState(openedFields.transferAccountId ?? "");
  const [error, setError] = useState("");
  const transfers = useMemo(() => budget.accounts.filter((a) => a.id !== account && !a.archived), [budget.accounts, account]);
  const save = () => {
    try {
      const next = opened.map((value, i) => values[i] === initial[i] ? value : values[i]);
      const fields: FieldChange[] = [];
      const moneyColumns = role.flatMap((r, i) => r === "amount" || r === "outflow" || r === "inflow" ? [i] : []);
      if (moneyColumns.some((i) => values[i] !== initial[i])) {
        let amount = 0;
        for (const i of moneyColumns) {
          const cents = evaluateAmount(values[i]);
          amount += role[i] === "outflow" ? -cents : cents;
          if (values[i] !== initial[i]) next[i] = values[i].trim() ? editableAmount(cents) : "";
        }
        if (!amount) throw new Error(t("Enter an amount other than zero."));
        if (!Number.isSafeInteger(amount) || Math.abs(amount) > 1_000_000_000_000) throw new Error(t("Amount is too large."));
        if (Math.round(Number(openedFields.amount) * 100) === amount) {
          // Equivalent math changes no transaction value. Keep the bank's original cells too,
          // rather than triggering another application of its sign or fee rules.
          for (const i of moneyColumns) next[i] = opened[i];
        } else fields.push({ field: "amount", from: openedFields.amount, to: (amount / 100).toFixed(2) });
      }
      if (transfer !== (openedFields.transferAccountId ?? "")) {
        if (transfer && !transfers.some((a) => a.id === transfer)) throw new Error(t("Choose an account."));
        fields.push({ field: "transferAccountId", from: openedFields.transferAccountId ?? "", to: transfer });
      }
      draft.current = null;
      onSave({ opened, values: next, fields });
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    }
  };
  draft.current = save;
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = row.current;
    const input = el?.querySelector<HTMLInputElement>(`[data-column="${focus}"] input, [data-column="${focus}"] select`);
    input?.focus({ preventScroll: true });
    if (input instanceof HTMLInputElement) input.select();
    const scroller = el?.closest<HTMLElement>(".line-scroll");
    const cell = input?.parentElement;
    if (!el || !scroller || !cell) return;
    // The cell may be off to the side: brought into view between the pinned columns (how the
    // line imports, what can be done to it), and the whole line above the bottom edge.
    const pinned = el.querySelector<HTMLElement>(".line-state")?.offsetWidth ?? 0;
    // Right to left, how the line imports is pinned on the right and its actions on the left; and
    // scrollLeft runs from 0, the right end, towards negative values, so the view's left edge
    // stands that far left of where it is at 0.
    const rtl = getDirection() === "rtl";
    const [left, right] = rtl ? [ACTIONS, pinned] : [pinned, ACTIONS];
    const shift = rtl ? scroller.scrollWidth - scroller.clientWidth : 0;
    const view = shift + scroller.scrollLeft;
    if (cell.offsetLeft - left < view) scroller.scrollLeft = cell.offsetLeft - left - shift;
    else if (cell.offsetLeft + cell.offsetWidth > view + scroller.clientWidth - right)
      scroller.scrollLeft = cell.offsetLeft + cell.offsetWidth - scroller.clientWidth + right - shift;
    const bottom = el.offsetTop + el.offsetHeight;
    if (bottom > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = bottom - scroller.clientHeight;
  }, []);
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.defaultPrevented) return;
    // Enter on a button does what the button does (Cancel cancels).
    if (e.key === "Enter" && !(e.target as HTMLElement).closest("button")) {
      e.preventDefault();
      save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };
  const problem = error || (line.status.kind === "attention" ? line.status.reason : line.status.kind === "leftOut" ? leftOutText(line) : "");
  const change = (i: number, text: string) => {
    setError("");
    const field = role[i];
    if (field === "outflow" || field === "inflow") {
      const entered = flowAmountChange(field, text);
      const other = role.indexOf(field === "outflow" ? "inflow" : "outflow");
      // A one-flow file has nowhere to move the sign: its explicit amount override still keeps the direction.
      if (entered.focus && other < 0) {
        setValues((v) => v.map((x, k) => k === i ? (field === "outflow" ? `-${text.slice(1)}` : text) : role[k] === "amount" ? "" : x));
      } else {
        setValues((v) => v.map((x, k) => {
          const r = role[k];
          return r === "outflow" || r === "inflow" ? entered.values[r] ?? x : r === "amount" && text ? "" : x;
        }));
        if (entered.focus) row.current?.querySelector<HTMLInputElement>(`[data-column="${other}"] input`)?.focus();
      }
    } else setValues((v) => v.map((x, k) => k === i ? text : field === "amount" && text && (role[k] === "outflow" || role[k] === "inflow") ? "" : x));
  };
  const chooseTransfer = (other: string) => {
    setError("");
    setTransfer(other);
    setValues((v) => v.map((x, k) => role[k] === "transferAccountId" ? other : role[k] === "payee" ? opened[k] : x));
  };
  return (
    <div className="line-row line-editor" role="row" ref={row} onKeyDown={keys} aria-label={t("Edit line {line}", { line: line.id })}>
      <span role="cell" className="line-state">
        <span className={problem ? "line-attention" : "line-editing"}>
          {problem ? <AlertTriangle size={14} aria-hidden /> : <Pencil size={13} aria-hidden />}
          <span role={error ? "alert" : undefined} title={problem}>{problem || t("Editing the line")}</span>
        </span>
      </span>
      {headers.map((h, i) => (
        <span role="cell" key={i} className="line-cell" data-column={i}>
          {role[i] === "payee" ? (
            <PayeeInput
              budget={budget}
              label={t("{column} of line {line}", { column: h || columnLabel(i), line: line.id })}
              value={transfer ? transferPayee(budget.accounts.find((a) => a.id === transfer)?.name ?? transfer) : values[i]}
              transfers={transfers}
              onTransfer={chooseTransfer}
              onChange={(text) => {
                change(i, text);
                setTransfer("");
                setValues((v) => v.map((x, k) => role[k] === "transferAccountId" ? "" : x));
              }}
            />
          ) : role[i] === "transferAccountId" ? (
            <select
              aria-label={t("{column} of line {line}", { column: h || columnLabel(i), line: line.id })}
              value={transfer}
              onChange={(e) => chooseTransfer(e.target.value)}
            >
              <option value="">{t("No transfer")}</option>
              {transfer && !transfers.some((a) => a.id === transfer) && <option value={transfer}>{transfer}</option>}
              {transfers.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
            </select>
          ) : <input
            aria-label={t("{column} of line {line}", { column: h || columnLabel(i), line: line.id })}
            value={values[i]}
            readOnly={locked[i]}
            title={
              locked[i]
                ? t("The bank's ID for the line isn't changed")
                : line.original?.length
                  ? t("In the file: {value}", { value: line.original[i] ?? "" })
                  : undefined
            }
            inputMode={role[i] === "amount" || role[i] === "outflow" || role[i] === "inflow" ? "decimal" : undefined}
            onChange={(e) => change(i, e.target.value)}
          />}
        </span>
      ))}
      <span role="cell" className="line-actions-cell" />
      <div className="line-editor-footer">
        <span className="line-editor-buttons">
          <button type="button" className="primary small" onClick={save}>
            {t("Save")}
          </button>
          <button type="button" className="secondary small" onClick={onCancel}>
            {t("Cancel")}
          </button>
          <span className="editor-hint" aria-hidden>
            {t("{enter} saves · {esc} cancels · {tab} moves to the next cell", {
              enter: <kbd>Enter</kbd>,
              esc: <kbd>Esc</kbd>,
              tab: <kbd>Tab</kbd>,
            })}
          </span>
        </span>
      </div>
    </div>
  );
}
