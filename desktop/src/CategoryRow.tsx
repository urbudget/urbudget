import { memo, useEffect, useRef, useState, type DragEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { ChevronDown, ChevronRight, CreditCard, GripVertical, History, MoonStar, MoreHorizontal, Plus } from "lucide-react";
import { deadline, editableAmount, evaluateAmount, money, ordinal } from "./format";
import { usePref } from "./prefs";
import type { Category, Summary } from "./types";
import { NEEDS_CATEGORY } from "./uncategorized";
import { HIDDEN_GROUP } from "./categories";
import { getLanguage, msg, t, tn, useLanguage } from "./i18n";
import { startEdge } from "./ui/Popover";

export type CategorySummary = Summary["categories"][number];
export type Drop = "before" | "after" | "into" | null;
/** Drag-and-drop handlers shared by every row (one stable object, so rows stay memoized). */
export type Dnd = {
  start: (kind: "category" | "group", id: string, e: DragEvent) => void;
  over: (kind: "category" | "group", id: string, e: DragEvent) => void;
  drop: (e: DragEvent) => void;
  end: () => void;
};

/** What a category needs this month, as YNAB shows it. */
export function categoryState(c: Category, s: CategorySummary, month: string) {
  const snoozed = String(c.goal?.goal_snoozed_at ?? "").startsWith(month);
  const balanceTarget = c.goal?.goal_type === "TB";
  const commitments = Math.max(0, -s.available, (s.upcoming ?? 0) - s.available);
  // Snoozing suppresses the target, not scheduled payments or money already overspent. An
  // undated balance asks for nothing this month either, as YNAB says: what it still lacks is
  // `eventually` ("2,000.00 more needed eventually"), and Underfunded leaves it out.
  const needed = snoozed || balanceTarget ? commitments : Math.max(commitments, s.needed ?? (c.target > 0 ? c.target - s.assigned : 0));
  const credit = (s.creditOverspending ?? 0) > 0 && (s.creditOverspending ?? 0) >= -s.available;
  // Native summaries report like-for-like funding for this month (or the balance for TB).
  // Older imported snapshots can omit it; use their assignment plus shortfall, never divide
  // a monthly shortfall by a weekly or annual target amount.
  const required = s.targetFundingRequired ?? (balanceTarget ? c.target : Math.max(0, s.assigned) + needed);
  const funded = s.targetFundingFunded ?? (balanceTarget ? s.available : Math.max(0, s.assigned));
  const eventually = balanceTarget && !snoozed ? Math.max(0, required - Math.max(0, funded)) : 0;
  const progress = c.target > 0 ? (required > 0 ? Math.min(100, Math.max(0, funded / required * 100)) : needed > 0 ? 0 : 100) : null;
  const status = credit
    ? "credit"
    : s.available < 0
      ? "negative"
      : needed > 0
        ? "underfunded"
        : s.available > 0
          ? "positive"
          : "zero";
  return { needed, eventually, snoozed, credit, progress, status };
}

/** A target's sentence: an English key with `{amount}` (unless `amount` is null) and its other values. */
export type TargetText = { amount: number | null; text: string; values: Record<string, string> };

/**
 * A category's target as YNAB words it: what's needed and by when ("120.00 needed by the 31st",
 * "20.00 more needed by 1 October", "45.00 more needed this month", "2,000.00 more needed
 * eventually" for a balance with no date), or "Funded" and "Fully spent" once the month's target
 * is met. The deadline comes from Rust's target_due (targets.rs). Overspending has its own
 * words, so it gets none here.
 */
export function targetText(
  c: Category,
  s: CategorySummary,
  state: ReturnType<typeof categoryState>,
): TargetText | null {
  if (state.status === "underfunded") {
    // A snoozed target asks for nothing, so what's needed is scheduled spending this month.
    const due = state.snoozed ? undefined : s.targetDue;
    const more = s.available > 0 || (s.targetFundingFunded ?? s.assigned) > 0;
    const amount = state.needed;
    if (!due) return { amount, text: more ? msg("{amount} more needed this month") : msg("{amount} needed this month"), values: {} };
    if (due.monthly) {
      // English says "the 31st"; Arabic just the day's number.
      const day = Number(due.date.slice(8, 10));
      const values = { day: getLanguage() === "en" ? ordinal(day) : String(day) };
      return { amount, text: more ? msg("{amount} more needed by the {day}") : msg("{amount} needed by the {day}"), values };
    }
    return { amount, text: more ? msg("{amount} more needed by {date}") : msg("{amount} needed by {date}"), values: { date: deadline(due.date) } };
  }
  if (state.status === "negative" || state.status === "credit") return null;
  if (state.eventually > 0)
    return {
      amount: state.eventually,
      text: s.available > 0 ? msg("{amount} more needed eventually") : msg("{amount} needed eventually"),
      values: {},
    };
  if (c.target <= 0 || state.snoozed || (state.progress ?? 0) < 100) return null;
  if (s.available > 0) return { amount: null, text: msg("Funded"), values: {} };
  return s.activity < 0 ? { amount: null, text: msg("Fully spent"), values: {} } : null;
}
/** targetText as one line, for a tooltip or a screen reader: "120.00 needed by the 31st". */
export const targetSentence = (target: TargetText) =>
  target.amount === null ? t(target.text) : t(target.text, { ...target.values, amount: money(target.amount) });
/** targetText with its amount shown as `amount` (bold in a row, strong in the inspector). */
export const targetLine = (target: TargetText, amount: ReactNode) =>
  target.amount === null ? t(target.text) : t(target.text, { ...target.values, amount });

/** Shows the budget's currency format; switches to the raw number while focused. Accepts math. Enter moves down. */
export function AmountInput({
  amount,
  onSave,
  label,
}: {
  amount: number;
  onSave: (amount: number) => void;
  label: string;
}) {
  const formatted = money(amount);
  const [value, setValue] = useState(formatted);
  const [focused, setFocused] = useState(false);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    if (!focused) setValue(formatted);
  }, [formatted, focused]);
  const commit = () => {
    try {
      const parsed = evaluateAmount(value);
      setInvalid(false);
      if (parsed !== amount) onSave(parsed);
    } catch {
      setInvalid(true);
    }
  };
  return (
    <input
      className={`amount-input${invalid ? " invalid" : ""}`}
      aria-label={label}
      aria-invalid={invalid}
      title={invalid ? t("Enter an amount like 12.50, or math like 10+2.50") : label}
      inputMode="decimal"
      value={value}
      onFocus={(e) => {
        const input = e.currentTarget;
        setFocused(true);
        setValue(editableAmount(amount));
        requestAnimationFrame(() => input.select());
      }}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => {
        commit();
        setFocused(false);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          const inputs = [...document.querySelectorAll<HTMLInputElement>(".amount-input")];
          const next = inputs[inputs.indexOf(e.currentTarget) + (e.shiftKey ? -1 : 1)];
          if (next) next.focus();
          else e.currentTarget.blur();
        }
        if (e.key === "Escape") {
          setValue(editableAmount(amount));
          setInvalid(false);
          requestAnimationFrame(() => (e.target as HTMLInputElement).blur());
        }
      }}
    />
  );
}

/** A name field: Enter or blur saves, Escape cancels. */
export function NameInput({
  value,
  label,
  onDone,
  placeholder,
}: {
  value: string;
  label: string;
  onDone: (name: string | null) => void;
  placeholder?: string;
}) {
  const [text, setText] = useState(value);
  const done = useRef(false);
  const finish = (name: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(name && name.trim() && name.trim() !== value ? name.trim() : null);
  };
  return (
    <input
      className="name-input"
      aria-label={label}
      autoFocus
      placeholder={placeholder}
      maxLength={200}
      value={text}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setText(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(text);
        if (e.key === "Escape") finish(null);
      }}
      onBlur={() => finish(text)}
    />
  );
}

const moveFocus = (e: KeyboardEvent<HTMLElement>, delta: number) => {
  const all = [...document.querySelectorAll<HTMLElement>(".budget-table [data-row-focus]")];
  all[all.indexOf(e.currentTarget) + delta]?.focus();
};

export const CategoryRow = memo(function CategoryRow({
  category,
  summary: s,
  month,
  selected,
  renaming,
  drop,
  dragging,
  dnd,
  onSelect,
  onAssign,
  onRename,
  onStartRename,
  onMenu,
  onMoveMoney,
  onActivity,
  moveCount,
  movedMoney,
  onMoves,
  onStep,
}: {
  category: Category;
  summary: CategorySummary;
  month: string;
  selected: boolean;
  renaming: boolean;
  drop: Drop;
  dragging: boolean;
  dnd: Dnd | null;
  onSelect: (id: string, e: { ctrl: boolean; shift: boolean }) => void;
  onAssign: (id: string, amount: number) => void;
  onRename: (id: string, name: string | null) => void;
  onStartRename: (id: string) => void;
  onMenu: (c: Category, x: number, y: number) => void;
  onMoveMoney: (c: Category, anchor: HTMLElement) => void;
  /** Shows the transactions behind the Activity amount. */
  onActivity: (c: Category, anchor: HTMLElement) => void;
  /** Itemised moves this month; the row offers them beside Assigned. */
  moveCount: number;
  /** Money moved between this and another category this month, which the row marks. */
  movedMoney: boolean;
  onMoves: (c: Category, anchor: HTMLElement) => void;
  onStep: (id: string, delta: -1 | 1) => void;
}) {
  // Memoized: follow a change of language too.
  useLanguage();
  const state = categoryState(category, s, month);
  const target = targetText(category, s, state);
  const [showNeeded] = usePref("showNeeded");
  // Set on every press in the row, so a press on the checkbox never starts a drag.
  const pressedBox = useRef(false);
  const menu = (e: MouseEvent) => {
    e.preventDefault();
    onMenu(category, e.clientX, e.clientY);
  };
  const canDrag = !!dnd && !renaming;
  const dragStart = (e: DragEvent) => {
    if (pressedBox.current) e.preventDefault();
    else dnd?.start("category", category.id, e);
  };
  // Overspending and scheduled commitments remain visible when a target is snoozed.
  const status = state.status === "negative" ? (
    <span className="row-status over">
      {t("{amount} overspent", { amount: <b>{money(-s.available)}</b> })}
    </span>
  ) : state.status === "credit" ? (
    <span className="row-status">
      {t("{amount} overspent on a card", { amount: <b>{money(-s.available)}</b> })}
    </span>
  ) : showNeeded && target?.amount != null ? (
    // Only this month's need is a warning; a balance with no date is needed "eventually".
    <span className={`row-status${state.status === "underfunded" ? "" : " later"}`} title={targetSentence(target)}>
      {targetLine(target, <b>{money(target.amount)}</b>)}
    </span>
  ) : state.snoozed ? (
    <span className="snoozed" title={t("Target snoozed this month")}>
      <MoonStar size={11} /> {t("Snoozed")}
    </span>
  ) : showNeeded && target ? (
    <span className="row-status">{targetSentence(target)}</span>
  ) : (
    <span className="row-status" />
  );
  return (
    <div
      role="row"
      aria-selected={selected}
      className={`budget-row${selected ? " selected" : ""}${dragging ? " dragging" : ""}${drop ? ` drop-${drop}` : ""}`}
      onClick={(e) => onSelect(category.id, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })}
      onContextMenu={menu}
      onDragOver={dnd ? (e) => dnd.over("category", category.id, e) : undefined}
      onDrop={dnd?.drop}
      onPointerDown={(e) => (pressedBox.current = (e.target as HTMLElement).classList.contains("row-select"))}
    >
      {/* The gutter holds the grip and the selection box. It drags the row, as the name does. */}
      <div
        role="gridcell"
        className="row-gutter"
        draggable={canDrag}
        onDragStart={canDrag ? dragStart : undefined}
        onDragEnd={dnd?.end}
      >
        <span className={`drag-handle${dnd ? " can-drag" : ""}`} aria-hidden title={dnd ? t("Drag to reorder") : undefined}>
          {dnd && <GripVertical size={14} />}
        </span>
        {/* Out of the tab order (Space on the name toggles instead), so rows keep one stop each. */}
        <input
          type="checkbox"
          className="row-select"
          tabIndex={-1}
          aria-label={t("Select {name}", { name: category.name })}
          checked={selected}
          // Focus stays where it was, so Escape and the arrow keys keep working after a click.
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => e.stopPropagation()}
          // A checkbox's change comes from a click, so Shift still selects a range.
          onChange={(e) => onSelect(category.id, { ctrl: true, shift: (e.nativeEvent as globalThis.MouseEvent).shiftKey })}
        />
      </div>
      {/* The whole name cell drags too, so a category can be picked up by its name. */}
      <div
        role="gridcell"
        className="category-cell"
        draggable={canDrag}
        onDragStart={canDrag ? dragStart : undefined}
        onDragEnd={dnd?.end}
      >
        {renaming ? (
          <NameInput value={category.name} label={t("Rename {name}", { name: category.name })} onDone={(name) => onRename(category.id, name)} />
        ) : (
          <button
            className="category-name"
            data-row-focus
            onDoubleClick={() => !category.paymentAccountId && onStartRename(category.id)}
            onKeyDown={(e) => {
              if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                e.preventDefault();
                onStep(category.id, e.key === "ArrowUp" ? -1 : 1);
              } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                e.preventDefault();
                moveFocus(e, e.key === "ArrowUp" ? -1 : 1);
              } else if (e.key === " ") {
                // Space toggles the row in the selection, as its checkbox does; Enter selects it alone.
                e.preventDefault();
                onSelect(category.id, { ctrl: true, shift: e.shiftKey });
              } else if (e.key === "F2" && !category.paymentAccountId) onStartRename(category.id);
              else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
                e.preventDefault();
                const r = e.currentTarget.getBoundingClientRect();
                onMenu(category, startEdge(r), r.bottom);
              }
            }}
            title={category.note ?? category.name}
          >
            <span className="name">{category.name}</span>
            {/* Meters line up across targets; without a meter the name can use its space. */}
            {state.progress !== null ? (
              <span
                className={`target-bar${state.status === "negative" ? " over" : state.progress >= 100 ? " funded" : ""}`}
                title={category.goal?.goal_type === "TB" ? (state.progress >= 100 ? t("Target balance reached") : t("{percent}% of target balance", { percent: Math.round(state.progress) })) : (state.progress >= 100 ? t("This month's target funding is complete") : t("{percent}% of this month's target funding", { percent: Math.round(state.progress) }))}
              >
                <i style={{ width: `${state.progress}%` }} />
              </span>
            ) : (
              <span className="target-bar none" />
            )}
            {status}
          </button>
        )}
      </div>
      <div
        role="gridcell"
        className={`assigned-cell${moveCount ? " has-moves" : ""}${movedMoney ? " moved" : ""}${s.assigned ? "" : " zero"}`}
        onClick={(e) => e.stopPropagation()}
      >
        {moveCount > 0 && (
          <button
            className="moves-button"
            aria-label={tn(moveCount, "{count} move in {name} this month", "{count} moves in {name} this month", { name: category.name })}
            title={movedMoney ? t("Money moved this month. Show moves") : t("Show this month's moves")}
            onClick={(e) => onMoves(category, e.currentTarget)}
          >
            <History size={13} />
          </button>
        )}
        <AmountInput label={t("Assigned to {name}", { name: category.name })} amount={s.assigned} onSave={(v) => onAssign(category.id, v)} />
      </div>
      <div role="gridcell" className={`num muted${s.activity ? "" : " zero"}`}>
        {s.activity ? (
          <button
            className="activity-button"
            aria-label={t("{amount} activity in {name}. Show transactions", { amount: money(s.activity), name: category.name })}
            title={category.paymentAccountId ? t("Open this card's transactions for the month") : t("Show these transactions")}
            onClick={(e) => {
              e.stopPropagation();
              onActivity(category, e.currentTarget);
            }}
          >
            {money(s.activity)}
          </button>
        ) : (
          money(s.activity)
        )}
      </div>
      <div role="gridcell" className="num">
        <button
          className={`available ${state.status}`}
          aria-label={
            state.status === "credit"
              ? t("{amount} available in {name}, overspent on a credit card. Move funds", { amount: money(s.available), name: category.name })
              : state.status === "underfunded" && target
                ? t("{amount} available in {name}, {target}. Move funds", { amount: money(s.available), name: category.name, target: targetSentence(target) })
                : t("{amount} available in {name}. Move funds", { amount: money(s.available), name: category.name })
          }
          title={
            state.status === "credit"
              ? t("Overspent on a credit card: this becomes card debt unless you cover it")
              : state.status === "negative"
                ? t("Overspent: cover it from another category")
                : state.status === "underfunded" && target
                  ? targetSentence(target)
                  : t("Move funds")
          }
          onClick={(e) => {
            e.stopPropagation();
            onMoveMoney(category, e.currentTarget);
          }}
        >
          {state.status === "credit" && <CreditCard size={12} />}
          {money(s.available)}
        </button>
      </div>
      {/* The row's menu, in a column of its own at the end, so nothing floats over the name. */}
      <div role="gridcell" className="row-actions">
        <button
          className="icon-button"
          aria-label={t("Actions for {name}", { name: category.name })}
          onClick={(e) => {
            e.stopPropagation();
            const r = e.currentTarget.getBoundingClientRect();
            onMenu(category, startEdge(r), r.bottom);
          }}
        >
          <MoreHorizontal size={16} />
        </button>
      </div>
    </div>
  );
});

/** Whether this month's uncategorized spending is debt on a card or cash, as a category's overspending. */
export const uncategorizedStatus = (activity: number, credit: number) => (credit > 0 && credit >= -activity ? "credit" : "negative");

/**
 * This month's spending with no category, as YNAB's Uncategorized row: overspending until it has
 * one. It isn't a category, so nothing is assigned to it; it opens those transactions.
 */
export function UncategorizedRow({ activity, credit, onReview }: { activity: number; credit: number; onReview?: () => void }) {
  const status = uncategorizedStatus(activity, credit);
  return (
    <div role="row" className="budget-row uncategorized-row">
      <div role="gridcell" className="row-gutter" />
      <div role="gridcell" className="category-cell">
        <button className="category-name" data-row-focus onClick={onReview} title={t("Show this month's spending that needs a category")}>
          <span className="name">{t("Uncategorized")}</span>
          <span className="target-bar none" />
          <span className="row-status">{t(NEEDS_CATEGORY)}</span>
        </button>
      </div>
      <div role="gridcell" className="num muted zero">
        {money(0)}
      </div>
      <div role="gridcell" className="num muted">
        {money(activity)}
      </div>
      <div role="gridcell" className="num">
        <button
          className={`available ${status}`}
          aria-label={
            status === "credit"
              ? t("{amount} uncategorized this month, overspent on a credit card. Show transactions", { amount: money(activity) })
              : t("{amount} uncategorized this month. Show transactions", { amount: money(activity) })
          }
          title={
            status === "credit"
              ? t("Spent on a card with no category: this is card debt until it has one")
              : t("Spent with no category: like overspending, it comes out of next month's Unassigned")
          }
          onClick={onReview}
        >
          {status === "credit" && <CreditCard size={12} />}
          {money(activity)}
        </button>
      </div>
      <div role="gridcell" className="row-actions" />
    </div>
  );
}

export function GroupRow({
  name,
  count,
  totals,
  open,
  editable,
  renaming,
  drop,
  dragging,
  dnd,
  movable,
  onToggle,
  onRename,
  onStartRename,
  onAdd,
  onMenu,
  onStep,
}: {
  name: string;
  count: number;
  totals: { assigned: number; activity: number; available: number };
  open: boolean;
  editable: boolean;
  renaming: boolean;
  drop: Drop;
  dragging: boolean;
  dnd: Dnd | null;
  /** Whether the group itself can be dragged (categories can still be dropped into it). */
  movable: boolean;
  onToggle: () => void;
  onRename: (name: string | null) => void;
  onStartRename: () => void;
  onAdd: () => void;
  onMenu: (x: number, y: number) => void;
  onStep: (delta: -1 | 1) => void;
}) {
  const canDrag = !!dnd && movable && !renaming;
  return (
    <div
      role="row"
      aria-expanded={open}
      className={`group-row${dragging ? " dragging" : ""}${drop ? ` drop-${drop}` : ""}`}
      onDragOver={dnd ? (e) => dnd.over("group", name, e) : undefined}
      onDrop={dnd?.drop}
      onContextMenu={(e) => {
        if (!editable) return;
        e.preventDefault();
        onMenu(e.clientX, e.clientY);
      }}
    >
      {/* The gutter holds the grip and the chevron, so group and category names share one edge. */}
      <div
        role="gridcell"
        className="row-gutter"
        draggable={canDrag}
        onDragStart={canDrag ? (e) => dnd.start("group", name, e) : undefined}
        onDragEnd={dnd?.end}
      >
        <span className={`drag-handle${canDrag ? " can-drag" : ""}`} aria-hidden title={canDrag ? t("Drag to reorder") : undefined}>
          {canDrag && <GripVertical size={14} />}
        </span>
        {/* The name button carries the state for keyboards and screen readers; this is a bigger
            target for the mouse. */}
        <span className="group-chevron" aria-hidden onClick={onToggle}>
          {open ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        </span>
      </div>
      <div
        role="rowheader"
        className="group-cell"
        draggable={canDrag}
        onDragStart={canDrag ? (e) => dnd.start("group", name, e) : undefined}
        onDragEnd={dnd?.end}
        // The band past the name folds the group too.
        onClick={(e) => e.target === e.currentTarget && onToggle()}
      >
        {renaming ? (
          <NameInput value={name} label={t("Rename group {name}", { name })} onDone={onRename} />
        ) : (
          <button
            className="group-name"
            data-row-focus
            aria-expanded={open}
            onClick={onToggle}
            onDoubleClick={() => editable && onStartRename()}
            onKeyDown={(e) => {
              if (e.altKey && editable && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                e.preventDefault();
                onStep(e.key === "ArrowUp" ? -1 : 1);
              } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                e.preventDefault();
                moveFocus(e, e.key === "ArrowUp" ? -1 : 1);
              } else if (e.key === "F2" && editable) onStartRename();
            }}
          >
            <span>{name === HIDDEN_GROUP ? t(HIDDEN_GROUP) : name}</span>
            <small className="group-count">{count}</small>
          </button>
        )}
        {editable && !renaming && (
          <button className="icon-button group-add" aria-label={t("Add a category to {name}", { name })} title={t("Add category")} onClick={onAdd}>
            <Plus size={14} />
          </button>
        )}
      </div>
      <div role="gridcell" className={`num${totals.assigned ? "" : " zero"}`}>
        {money(totals.assigned)}
      </div>
      <div role="gridcell" className={`num${totals.activity ? "" : " zero"}`}>
        {money(totals.activity)}
      </div>
      <div role="gridcell" className={`num${totals.available ? "" : " zero"}`}>
        {money(totals.available)}
      </div>
      <div role="gridcell" className="row-actions">
        {editable && !renaming && (
          <button
            className="icon-button"
            aria-label={t("Actions for group {name}", { name })}
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              onMenu(startEdge(r), r.bottom);
            }}
          >
            <MoreHorizontal size={16} />
          </button>
        )}
      </div>
    </div>
  );
}
