import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type DragEvent, type RefObject } from "react";
import {
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  EyeOff,
  Eye,
  FolderPlus,
  History,
  Info,
  Pencil,
  Plus,
  Sparkles,
  Target,
  Trash2,
} from "lucide-react";
import { api } from "./bridge";
import { CommitInput, Modal } from "./forms";
import { HIDDEN_GROUP, groupNames, orderedCategories, orderedGroups } from "./categories";
import { errorText, money, monthName, tone } from "./format";
import { uncategorizedCounts } from "./uncategorized";
import { CategoryRow, GroupRow, NameInput, UncategorizedRow, categoryState, type CategorySummary, type Dnd, type Drop } from "./CategoryRow";
import { BudgetInspector } from "./BudgetInspector";
import { GettingStarted } from "./Welcome";
import { autoAssignAction } from "./QuickAssign";
import { MoveMoneyPopover } from "./MoneyPopovers";
import { ActivityPopover } from "./ActivityPopover";
import { MovesPopover } from "./MovesPopover";
import { monthMoveStats } from "./moves";
import { usePref } from "./prefs";
import {
  addCategory,
  addGroup,
  assignAmount,
  deleteCategory,
  deleteGroup,
  moveCategory,
  moveGroup,
  renameCategory,
  renameGroup,
  setHidden,
  stepCategory,
} from "./budgetEdits";
import {
  CategoryPicker,
  Menu,
  READY_TO_ASSIGN,
  Tabs,
  usePinned,
  useToast,
  type Anchor,
  type MenuItem,
} from "./ui";
import type { Budget, Category, Summary } from "./types";
import { textWidth } from "./ui/columns";
import { t, tn } from "./i18n";
import "./budget.css";

/** The last day of a month ("2026-02" → "2026-02-28"). */
const monthEnd = (month: string) => {
  const [y, m] = month.split("-").map(Number);
  return `${month}-${String(new Date(y, m, 0).getDate()).padStart(2, "0")}`;
};

/**
 * Age of money for the month, worked out in the background after transactions change.
 * Keeps showing the last value while it updates, so switching months doesn't flicker.
 */
function useAgeOfMoney(budget: Budget, month: string, enabled: boolean) {
  const [age, setAge] = useState<number | null | undefined>(undefined);
  const latest = useRef(budget);
  latest.current = budget;
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      api
        .reports(latest.current, { from: month, to: month, accountIds: [], categoryIds: [] }, controller.signal)
        .then((r) => !cancelled && setAge(r.ageOfMoney))
        .catch(() => !cancelled && setAge(null));
    }, 250);
    return () => {
      cancelled = true;
      controller.abort();
      clearTimeout(timer);
    };
  }, [budget.transactions, budget.accounts, month, enabled]);
  return age;
}

type Edit = (update: (draft: Budget) => void) => void;
type View = "all" | "underfunded" | "overfunded" | "available" | "overspent" | "snoozed";
const blank = (c: Category): CategorySummary => ({
  id: c.id,
  assigned: 0,
  activity: 0,
  available: 0,
  carried: 0,
  needed: c.target,
  creditOverspending: 0,
});
const store = {
  get(key: string): string[] | null {
    try {
      return JSON.parse(localStorage.getItem(key) ?? "null");
    } catch {
      return null;
    }
  },
  set(key: string, value: string[]) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* not remembered */
    }
  },
};

function MonthNote({ budget, month, edit }: { budget: Budget; month: string; edit: Edit }) {
  const saved = budget.ynabMonths?.find((m) => m.month.startsWith(month))?.note ?? "";
  return (
    <CommitInput
      className="month-note"
      aria-label={t("Month note")}
      placeholder={t("Note for {month}…", { month: monthName(month, "short") })}
      value={saved}
      onCommit={(note) =>
        edit((d) => {
          d.ynabMonths ??= [];
          const entry = d.ynabMonths.find((m) => m.month.startsWith(month));
          if (entry) entry.note = note;
          else d.ynabMonths.push({ month: `${month}-01`, note });
        })
      }
    />
  );
}

export function BudgetPage({
  budget,
  budgetId = "",
  summary,
  month,
  edit,
  transform,
  undo,
  onEditorChange,
  onEditCategory,
  onNewCategory,
  onAddAccount,
  onAddTransaction,
  onOpenSample,
  onOpenRegister,
  onAsk,
  onPayCard,
}: {
  /** Opens the assistant about these categories (by name). */
  onAsk?: (names: string[]) => void;
  /** Pays a credit card (by account id), from its payment category in the inspector. */
  onPayCard?: (accountId: string) => void;
  /** Opens a register (an account, or "all") filtered to a category and dates, or to spending
   * that needs a category. */
  onOpenRegister?: (target: { accountId: string; category?: string; from?: string; to?: string; uncategorized?: boolean }) => void;
  budget: Budget;
  budgetId?: string;
  summary: Summary;
  month: string;
  edit: Edit;
  transform: (fn: (b: Budget) => Promise<Budget>) => Promise<void>;
  undo?: () => void;
  onEditorChange: (open: boolean) => void;
  onEditCategory: (c: Category) => void;
  onNewCategory?: () => void;
  onAddAccount: () => void;
  /** Opens the new-transaction dialog (the getting-started checklist offers it). */
  onAddTransaction?: () => void;
  onOpenSample: () => void;
}) {
  const toast = useToast();
  const collapsedKey = `lb:budget:collapsed:${budgetId}`;
  const [view, setView] = useState<View>("all");
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set(store.get(collapsedKey) ?? [HIDDEN_GROUP]));
  const [selected, setSelected] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);
  const [renaming, setRenaming] = useState<{ kind: "category" | "group"; id: string } | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const [addingGroup, setAddingGroup] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[]; label: string } | null>(null);
  const [mover, setMover] = useState<{
    source: string;
    name: string;
    available: number;
    anchor: RefObject<HTMLElement | null>;
  } | null>(null);
  const [deleting, setDeleting] = useState<
    { kind: "category"; category: Category } | { kind: "group"; name: string } | null
  >(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const [targetFor, setTargetFor] = useState<string | null>(null);
  const [activity, setActivity] = useState<{ category: Category; anchor: RefObject<HTMLElement | null> } | null>(null);
  const [moves, setMoves] = useState<{ category: Category; anchor: Anchor } | null>(null);
  const [showAgeOfMoney] = usePref("showAgeOfMoney");
  const ageOfMoney = useAgeOfMoney(budget, month, showAgeOfMoney);
  const [drop, setDrop] = useState<{ kind: "category" | "group"; id: string; where: Drop } | null>(null);

  useEffect(() => setCollapsed(new Set(store.get(collapsedKey) ?? [HIDDEN_GROUP])), [collapsedKey]);
  const editing = !!(renaming || adding || addingGroup || mover || deleting || targetFor);
  useEffect(() => {
    onEditorChange(editing);
    return () => onEditorChange(false);
  }, [editing, onEditorChange]);

  const summaries = useMemo(() => new Map(summary.categories.map((c) => [c.id, c])), [summary.categories]);
  const moveStats = useMemo(() => monthMoveStats(budget, month), [budget.moneyMoves, month]);
  const moveStatsRef = useRef(moveStats);
  moveStatsRef.current = moveStats;
  const available = useMemo(() => new Map(summary.categories.map((c) => [c.id, c.available])), [summary.categories]);
  const groups = useMemo(
    () => orderedGroups(budget, { empty: true }),
    [budget.categories, budget.categoryOrder, budget.groupOrder],
  );
  const matches = useCallback(
    (c: Category, v: View) => {
      if (v === "all") return true;
      const s = summaries.get(c.id) ?? blank(c);
      const st = categoryState(c, s, month);
      return v === "underfunded"
        ? st.status === "underfunded"
        : v === "overfunded"
          ? c.target > 0 && s.available > c.target
          : v === "available"
            ? s.available > 0
            : v === "overspent"
              ? s.available < 0
              : st.snoozed;
    },
    [summaries, month],
  );
  const counts = useMemo(() => {
    const visible = budget.categories.filter((c) => !c.hidden);
    const count = (v: View) => visible.filter((c) => matches(c, v)).length;
    return {
      underfunded: count("underfunded"),
      overfunded: count("overfunded"),
      available: count("available"),
      overspent: count("overspent"),
      snoozed: count("snoozed"),
    };
  }, [budget.categories, matches]);
  const shown = useMemo(
    () => groups
      .map((g) => ({ ...g, categories: g.categories.filter((c) => matches(c, view)) }))
      .filter((g) => view === "all" || g.categories.length),
    [groups, matches, view],
  );
  // The engine keeps Uncategorized separate from category balances for RTA accounting.
  // Display it alongside the categories in the whole month's Activity and Available totals.
  const monthUncategorized = summary.uncategorized ?? 0;
  const monthActivity = summary.activity + monthUncategorized;
  const monthAvailable = summary.available + monthUncategorized;
  const amountColumns = useMemo(() => {
    const keys = ["assigned", "activity", "available"] as const;
    const low = { assigned: 0, activity: 0, available: 0 };
    const high = { assigned: 0, activity: 0, available: 0 };
    const include = (values: Pick<CategorySummary, (typeof keys)[number]>) => {
      for (const key of keys) {
        low[key] = Math.min(low[key], values[key]);
        high[key] = Math.max(high[key], values[key]);
      }
    };
    include({ assigned: summary.assigned, activity: monthActivity, available: monthAvailable });
    include({ assigned: 0, activity: monthUncategorized, available: monthUncategorized });
    for (const group of shown) {
      const totals = { assigned: 0, activity: 0, available: 0 };
      for (const category of group.categories) {
        const values = summaries.get(category.id);
        if (!values) continue;
        include(values);
        for (const key of keys) totals[key] += values[key];
      }
      include(totals);
    }
    // Only the extremes need measuring: tabular digits have equal width. Leave room for
    // cell padding, the Assigned history button, and the Available credit-card icon.
    const padding = { assigned: 48, activity: 28, available: 52 };
    return Object.fromEntries(keys.map((key) => [
      `--${key}-width`,
      `${Math.max(textWidth(money(low[key]), 13, 650, true), textWidth(money(high[key]), 13, 650, true)) + padding[key]}px`,
    ])) as CSSProperties;
  }, [shown, summaries, summary.assigned, monthActivity, monthAvailable, monthUncategorized, budget.currency, budget.numberFormat]);
  const visibleOrder = shown.flatMap((g) => (collapsed.has(g.name) ? [] : g.categories.map((c) => c.id)));
  const visibleIds = useMemo(
    () => budget.categories.filter((c) => !c.hidden).map((c) => c.id),
    [budget.categories],
  );
  const realGroups = groupNames(budget, { empty: true });
  const allChosen = visibleOrder.length > 0 && visibleOrder.every((id) => selected.includes(id));

  const persistCollapsed = (next: Set<string>) => {
    setCollapsed(next);
    store.set(collapsedKey, [...next]);
  };
  const toggle = (group: string) => {
    const next = new Set(collapsed);
    if (!next.delete(group)) next.add(group);
    persistCollapsed(next);
  };
  const undoToast = (message: string) =>
    toast({ message, action: undo ? { label: t("Undo"), onClick: undo } : undefined });

  // Stable callbacks, so memoized rows only re-render when their own data changes.
  const monthRef = useRef(month);
  monthRef.current = month;
  const orderRef = useRef(visibleOrder);
  orderRef.current = visibleOrder;
  const select = useCallback((id: string, e: { ctrl: boolean; shift: boolean }) => {
    setSelected((current) => {
      if (e.shift && anchor.current) {
        const order = orderRef.current;
        const [a, b] = [order.indexOf(anchor.current), order.indexOf(id)].sort((x, y) => x - y);
        return a < 0 ? [id] : order.slice(a, b + 1);
      }
      anchor.current = id;
      if (e.ctrl) return current.includes(id) ? current.filter((x) => x !== id) : [...current, id];
      return current.length === 1 && current[0] === id ? current : [id];
    });
  }, []);
  const assign = useCallback(
    (categoryId: string, amount: number) => {
      const month = monthRef.current;
      edit((d) => assignAmount(d, month, categoryId, amount));
    },
    [edit],
  );
  const rename = useCallback(
    (id: string, name: string | null) => {
      setRenaming(null);
      if (name) edit((d) => renameCategory(d, id, name));
    },
    [edit],
  );
  const startRename = useCallback((id: string) => setRenaming({ kind: "category", id }), []);
  const step = useCallback(
    (id: string, delta: -1 | 1) => {
      edit((d) => stepCategory(d, id, delta));
      requestAnimationFrame(() =>
        document.querySelector<HTMLElement>(`[data-category="${id}"] [data-row-focus]`)?.focus(),
      );
    },
    [edit],
  );
  const undoRef = useRef(undo);
  undoRef.current = undo;
  const categoryMenu = useCallback(
    (c: Category, x: number, y: number) => {
      const withUndo = (message: string) =>
        toast({ message, action: undoRef.current ? { label: t("Undo"), onClick: undoRef.current } : undefined });
      const items: MenuItem[] = [
        ...(!c.paymentAccountId
          ? [{ label: t("Rename"), icon: <Pencil size={14} />, hint: "F2", onSelect: () => setRenaming({ kind: "category", id: c.id }) }]
          : []),
        {
          label: c.target > 0 ? t("Edit target…") : t("Create target…"),
          icon: <Target size={14} />,
          onSelect: () => {
            setSelected([c.id]);
            setTargetFor(c.id);
          },
        },
        ...(moveStatsRef.current.get(c.id)?.count
          ? [{ label: t("Show moves"), icon: <History size={14} />, onSelect: () => setMoves({ category: c, anchor: { x, y } }) }]
          : []),
        c.hidden
          ? { label: t("Show category"), icon: <Eye size={14} />, onSelect: () => edit((d) => setHidden(d, [c.id], false)) }
          : {
              label: t("Hide category"),
              icon: <EyeOff size={14} />,
              onSelect: () => {
                edit((d) => setHidden(d, [c.id], true));
                withUndo(t("{name} is hidden. Find it under Hidden categories.", { name: c.name }));
              },
            },
        ...(!c.paymentAccountId
          ? ([
              "separator",
              { label: t("Delete…"), icon: <Trash2 size={14} />, danger: true, onSelect: () => setDeleting({ kind: "category", category: c }) },
            ] as MenuItem[])
          : []),
      ];
      setMenu({ x, y, items, label: t("Actions for {name}", { name: c.name }) });
    },
    [edit, toast],
  );
  const openRegisterRef = useRef(onOpenRegister);
  openRegisterRef.current = onOpenRegister;
  const showActivity = useCallback((c: Category, el: HTMLElement) => {
    const m = monthRef.current;
    // A card payment category's activity is the card's spending and payments: show the card.
    if (c.paymentAccountId) openRegisterRef.current?.({ accountId: c.paymentAccountId, from: `${m}-01`, to: monthEnd(m) });
    else setActivity({ category: c, anchor: { current: el } });
  }, []);
  const summariesRef = useRef(summaries);
  summariesRef.current = summaries;
  const openMover = useCallback(
    (c: Category, el: HTMLElement) =>
      setMover({ source: c.id, name: c.name, available: summariesRef.current.get(c.id)?.available ?? 0, anchor: { current: el } }),
    [],
  );
  const showMoves = useCallback((c: Category, el: HTMLElement) => setMoves({ category: c, anchor: { current: el } }), []);

  // Drag and drop: categories within and across groups, groups among groups.
  const dragRef = useRef<{ kind: "category" | "group"; id: string } | null>(null);
  const dropRef = useRef(drop);
  dropRef.current = drop;
  const budgetRef = useRef(budget);
  budgetRef.current = budget;
  const dnd: Dnd = useMemo(
    () => ({
      start: (kind, id, e) => {
        dragRef.current = { kind, id };
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", id);
        const row = (e.target as HTMLElement).closest("[role=row]");
        if (row) e.dataTransfer.setDragImage(row, 16, 16);
        setDragging(id);
      },
      over: (kind, id, e: DragEvent) => {
        const d = dragRef.current;
        if (!d || (d.kind === "group" && kind !== "group") || d.id === id || id === HIDDEN_GROUP) return;
        if (kind === "category" && budgetRef.current.categories.find((c) => c.id === id)?.hidden) return;
        e.preventDefault();
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        const where: Drop =
          d.kind === "category" && kind === "group" ? "into" : e.clientY > r.top + r.height / 2 ? "after" : "before";
        if (dropRef.current?.id !== id || dropRef.current.where !== where) setDrop({ kind, id, where });
      },
      drop: (e) => {
        e.preventDefault();
        const d = dragRef.current,
          target = dropRef.current;
        if (d && target)
          edit((draft) => {
            if (d.kind === "group") {
              const order = groupNames(draft, { empty: true }).filter((g) => g !== d.id);
              const before = target.where === "before" ? target.id : (order[order.indexOf(target.id) + 1] ?? null);
              moveGroup(draft, d.id, before);
            } else if (target.kind === "group") moveCategory(draft, d.id, target.id, null);
            else {
              const over = draft.categories.find((c) => c.id === target.id)!;
              const siblings = (orderedGroups(draft).find((g) => g.name === over.group)?.categories ?? []).filter(
                (c) => c.id !== d.id,
              );
              const next = siblings[siblings.findIndex((c) => c.id === over.id) + 1];
              moveCategory(draft, d.id, over.group, target.where === "before" ? over.id : (next?.id ?? null));
            }
          });
        dragRef.current = null;
        setDrop(null);
        setDragging(null);
      },
      end: () => {
        dragRef.current = null;
        setDrop(null);
        setDragging(null);
      },
    }),
    [edit],
  );

  useEffect(() => {
    if (!selected.length) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !(e.target as HTMLElement).closest?.("input,select,textarea,.modal,.popover"))
        setSelected([]);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [selected.length]);

  const moveMoney = async (from: string, to: string, amount: number) => {
    await transform((b) => api.action(b, { type: "moveMoney", month, from, to, amount }));
    const name = (id: string) =>
      id === READY_TO_ASSIGN ? t("Unassigned") : (budget.categories.find((c) => c.id === id)?.name ?? id);
    undoToast(t("Moved {amount} from {from} to {to}.", { amount: money(amount, { symbol: true }), from: name(from), to: name(to) }));
  };
  const autoAssign = async (mode: string, label: string, ids: string[], all: boolean, change: number, asOfMonth: string) => {
    try {
      await transform((b) => api.action(b, autoAssignAction(mode, month, ids, all, asOfMonth)));
      const amount = money(Math.abs(change), { symbol: true });
      undoToast(change >= 0 ? t("{label}: assigned {amount}.", { label, amount }) : t("{label}: removed {amount}.", { label, amount }));
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    }
  };

  const rta = summary.readyToAssign;
  // Spending with no category yet: overspending until it has one, as in YNAB (uncategorized.ts).
  const uncategorized = useMemo(
    () => uncategorizedCounts(budget),
    [budget.transactions, budget.accounts, budget.categories],
  );
  // This month's share, as YNAB's Uncategorized row, in the views overspending shows in.
  const showUncategorized = monthUncategorized < 0 && (view === "all" || view === "overspent");
  const reviewUncategorized = onOpenRegister
    ? () => onOpenRegister({ accountId: "all", uncategorized: true, from: `${month}-01`, to: monthEnd(month) })
    : undefined;
  const rtaButton = useRef<HTMLButtonElement>(null);
  const pinned = usePinned();
  const selectedCategories = selected
    .map((id) => budget.categories.find((c) => c.id === id))
    .filter((c): c is Category => !!c);

  const gettingStarted = (
    <GettingStarted
      budget={budget}
      budgetId={budgetId}
      onAddAccount={onAddAccount}
      onAssign={() => rtaButton.current?.click()}
      onAddTransaction={onAddTransaction}
      onSetTarget={() => {
        // The first everyday category, with its target editor open.
        const category = orderedCategories(budget).find((c) => !c.hidden && !c.paymentAccountId);
        if (!category) return onNewCategory?.();
        setSelected([category.id]);
        setTargetFor(category.id);
      }}
    />
  );
  if (!budget.accounts.length)
    return (
      <div className="page">
        {gettingStarted}
        <div className="empty-state">
          <h2>{t("No accounts yet")}</h2>
          <p>{t("Add your accounts and their current balances, then create categories and assign the money you have.")}</p>
          <div className="button-row">
            <button className="primary" onClick={onAddAccount}>
              <Plus size={15} /> {t("Add account")}
            </button>
            <button className="secondary" onClick={onOpenSample}>
              <Sparkles size={15} /> {t("Explore a sample budget")}
            </button>
          </div>
        </div>
      </div>
    );

  const groupMenu = (name: string, categories: Category[]): MenuItem[] => [
    {
      label: t("Rename group"),
      icon: <Pencil size={14} />,
      hint: "F2",
      onSelect: () => setRenaming({ kind: "group", id: name }),
    },
    { label: t("Add category"), icon: <Plus size={14} />, onSelect: () => setAdding(name) },
    ...(categories.length
      ? [
          {
            label: t("Hide all categories"),
            icon: <EyeOff size={14} />,
            onSelect: () => edit((d) => setHidden(d, categories.map((c) => c.id), true)),
          },
        ]
      : []),
    "separator",
    {
      label: t("Delete group…"),
      icon: <Trash2 size={14} />,
      danger: true,
      onSelect: () => setDeleting({ kind: "group", name }),
    },
  ];
  const renameGroupTo = (name: string, to: string | null) => {
    setRenaming(null);
    if (!to) return;
    const merging = realGroups.includes(to);
    edit((d) => renameGroup(d, name, to));
    if (merging) undoToast(t("Merged {name} into {to}.", { name, to }));
  };
  const addCategoryTo = (group: string, name: string | null) => {
    setAdding(null);
    if (name) edit((d) => void addCategory(d, group, name));
  };
  const addGroupNamed = (name: string | null) => {
    setAddingGroup(false);
    if (!name) return;
    if (realGroups.includes(name)) {
      toast({ message: t("There is already a group called {name}.", { name }), tone: "error" });
      return;
    }
    edit((d) => void addGroup(d, name));
    setAdding(name);
  };

  const tabs: { id: View; label: string; count?: number }[] = [
    { id: "all", label: t("All") },
    { id: "underfunded", label: t("Underfunded"), count: counts.underfunded },
    { id: "overfunded", label: t("Overfunded"), count: counts.overfunded },
    { id: "available", label: t("Money available"), count: counts.available },
    { id: "overspent", label: t("Overspent"), count: counts.overspent },
    { id: "snoozed", label: t("Snoozed"), count: counts.snoozed },
  ];
  // One toggle: collapse every group, or expand them all once they're collapsed.
  const foldable = groups.filter((g) => g.name !== HIDDEN_GROUP);
  const folded = foldable.length > 0 && foldable.every((g) => collapsed.has(g.name));
  const foldAll = () => persistCollapsed(new Set(folded ? [HIDDEN_GROUP] : groups.map((g) => g.name)));
  return (
    <div className="budget-layout">
      {/* The column scrolls as one, so its scrollbar runs from the top; the summary and toolbar
          stay pinned above the table, whose header and total stick while the rows scroll. */}
      <div className="budget-main">
        <div className="budget-pinned" ref={pinned.ref}>
          {gettingStarted}
          <div className="summary-strip">
            <button
              ref={rtaButton}
              className={`rta ${tone(rta)}`}
              title={rta >= 0 ? t("Assign this money") : t("Take money back from categories")}
              onClick={() => setMover({ source: READY_TO_ASSIGN, name: t("Unassigned"), available: rta, anchor: rtaButton })}
            >
              <span>{t("Unassigned")}</span>
              <strong>{money(rta, { symbol: true })}</strong>
              <small className="rta-hint">
                {rta < 0
                  ? t("More is assigned than you have")
                  : rta === 0
                    ? t("All of it is assigned")
                    : t("Select to assign")}
              </small>
            </button>
            <dl className="stats">
              <div>
                <dt>{t("Assigned")}</dt>
                <dd>{money(summary.assigned)}</dd>
              </div>
              <div>
                <dt title={t("Changes to category balances, including uncategorized spending and credit-card payment reserves. See Reports for spending totals.")}>{t("Activity")}</dt>
                <dd>{money(monthActivity)}</dd>
              </div>
              <div>
                <dt>{t("Available")}</dt>
                <dd>{money(monthAvailable)}</dd>
              </div>
              {showAgeOfMoney && (
                <div title={t("How long your money waits before you spend it: the average age, in days, of the money in your last 10 payments from cash accounts, spending the oldest money first. Until there are 10, the missing ones count as 0 days. An estimate.")}>
                  <dt>{t("Cash age")}</dt>
                  <dd>{ageOfMoney === undefined ? "…" : ageOfMoney === null ? "—" : tn(ageOfMoney, "{count} day", "{count} days")}</dd>
                </div>
              )}
            </dl>
          </div>
          {/* The engine lists each short future month, from the calculation month on. Before it,
              money that came in later funds the later months, so a past month lists none. */}
          {(!!summary.futureAssigned || !!summary.futureShortfalls?.length) && (
            <div className={`notice${summary.futureShortfalls?.length ? " error" : ""}`}>
              {!!summary.futureAssigned && <>{t("Assigned in future months: {amount}.", { amount: money(summary.futureAssigned, { symbol: true }) })} </>}
              {summary.futureShortfalls?.length
                ? <>{summary.futureShortfalls.map((s) => t("{month} is short by {amount}.", { month: monthName(s.month), amount: money(s.amount) })).join(" ")} {t("Move funds back from a future month.")}</>
                : (summary.futureAssigned ?? 0) > 0 && (summary.futureReadyToAssign ?? 0) + (summary.futureAssigned ?? 0) >= 0
                  && t("It is held back from this month's unassigned money.")}
            </div>
          )}
          {uncategorized.total > 0 && (
            // As the register marks them: the attention colour, and what it costs until it's fixed.
            <div className="needs-banner" role="status">
              <Info size={18} aria-hidden />
              <p>
                <strong>
                  {tn(uncategorized.total, "{count} transaction needs a category", "{count} transactions need a category")}
                </strong>
                <span>
                  {tn(
                    uncategorized.total,
                    "{amount} counts as overspending until it has one.",
                    "{amount} counts as overspending until they have one.",
                    { amount: money(uncategorized.amount, { symbol: true }) },
                  )}
                </span>
              </p>
              {onOpenRegister && (
                <button className="needs-banner-review" onClick={() => onOpenRegister({ accountId: "all", uncategorized: true })}>
                  {t("Review")}
                  <ChevronRight size={15} aria-hidden />
                </button>
              )}
            </div>
          )}
          <div className="budget-toolbar">
            <Tabs<View>
              label={t("Show categories")}
              value={view}
              onChange={setView}
              tabs={tabs.filter((tab) => tab.id === "all" || tab.id === view || tab.count)}
            />
            <MonthNote budget={budget} month={month} edit={edit} />
            <span className="toolbar-buttons">
              <button
                className="secondary"
                title={folded ? t("Show every group's categories") : t("Show only group totals")}
                onClick={foldAll}
              >
                {folded ? <ChevronsUpDown size={15} /> : <ChevronsDownUp size={15} />}
                {folded ? t("Expand all") : t("Collapse all")}
              </button>
              <button className="secondary" onClick={() => setAddingGroup(true)}>
                <FolderPlus size={15} /> {t("Group")}
              </button>
            </span>
          </div>
        </div>
        <div className="budget-table" style={amountColumns} role="grid" aria-label={t("Monthly plan")} aria-multiselectable>
          <div className="budget-head" role="row">
            <span role="columnheader" className="row-gutter">
              {/* Selects every category on screen; a dash when only some are selected. */}
              <input
                type="checkbox"
                className="row-select"
                aria-label={t("Select all categories")}
                checked={allChosen}
                disabled={!visibleOrder.length}
                ref={(el) => {
                  if (el) el.indeterminate = selected.length > 0 && !allChosen;
                }}
                onChange={() => setSelected(allChosen ? [] : visibleOrder)}
              />
            </span>
            <span role="columnheader" className="category-cell">
              <span className="category-name">
                <span className="name">{t("Category")}</span>
                <span />
                <span className="row-status">{t("Target")}</span>
              </span>
            </span>
            <span role="columnheader" className="num">
              {t("Assigned")}
            </span>
            <span role="columnheader" className="num">
              {t("Activity")}
            </span>
            <span role="columnheader" className="num">
              {t("Available")}
            </span>
            <span aria-hidden className="row-actions" />
          </div>
          {addingGroup && (
            <div className="group-row adding" role="row">
              <span className="row-gutter" />
              <div className="group-cell">
                <NameInput value="" label={t("New group name")} placeholder={t("New group name")} onDone={addGroupNamed} />
              </div>
            </div>
          )}
          {showUncategorized && (
            <UncategorizedRow activity={monthUncategorized} credit={summary.uncategorizedCredit ?? 0} onReview={reviewUncategorized} />
          )}
          {shown.map(({ name, categories }) => {
            const totals = categories.reduce(
              (t, c) => {
                const s = summaries.get(c.id);
                return s
                  ? { assigned: t.assigned + s.assigned, activity: t.activity + s.activity, available: t.available + s.available }
                  : t;
              },
              { assigned: 0, activity: 0, available: 0 },
            );
            const real = name !== HIDDEN_GROUP;
            const closed = collapsed.has(name);
            return (
              <div key={name} className="budget-group" role="rowgroup">
                <GroupRow
                  name={name}
                  count={categories.length}
                  totals={totals}
                  open={!closed}
                  editable={real}
                  renaming={renaming?.kind === "group" && renaming.id === name}
                  drop={drop?.kind === "group" && drop.id === name ? drop.where : null}
                  dragging={dragging === name}
                  dnd={real && view === "all" ? dnd : null}
                  movable={real}
                  onToggle={() => toggle(name)}
                  onStartRename={() => setRenaming({ kind: "group", id: name })}
                  onRename={(to) => renameGroupTo(name, to)}
                  onAdd={() => {
                    if (closed) toggle(name);
                    setAdding(name);
                  }}
                  onStep={(delta) => {
                    const i = realGroups.indexOf(name);
                    if (delta < 0 && i <= 0) return;
                    const before = delta < 0 ? realGroups[i - 1] : (realGroups[i + 2] ?? null);
                    edit((d) => moveGroup(d, name, before));
                  }}
                  onMenu={(x, y) => setMenu({ x, y, label: t("Actions for group {name}", { name }), items: groupMenu(name, categories) })}
                />
                {!closed &&
                  categories.map((c) => (
                    <div key={c.id} data-category={c.id}>
                      <CategoryRow
                        category={c}
                        summary={summaries.get(c.id) ?? blank(c)}
                        month={month}
                        selected={selected.includes(c.id)}
                        renaming={renaming?.kind === "category" && renaming.id === c.id}
                        drop={drop?.kind === "category" && drop.id === c.id ? drop.where : null}
                        dragging={dragging === c.id}
                        dnd={view === "all" && real ? dnd : null}
                        onSelect={select}
                        onAssign={assign}
                        onRename={rename}
                        onStartRename={startRename}
                        onMenu={categoryMenu}
                        onMoveMoney={openMover}
                        onActivity={showActivity}
                        moveCount={moveStats.get(c.id)?.count ?? 0}
                        movedMoney={moveStats.get(c.id)?.moved ?? false}
                        onMoves={showMoves}
                        onStep={step}
                      />
                    </div>
                  ))}
                {adding === name && (
                  <div className="budget-row adding" role="row">
                    <span className="row-gutter" />
                    <div className="category-cell">
                      <NameInput
                        value=""
                        label={t("New category in {name}", { name })}
                        placeholder={t("New category name")}
                        onDone={(categoryName) => addCategoryTo(name, categoryName)}
                      />
                    </div>
                  </div>
                )}
                {real && !closed && !categories.length && adding !== name && view === "all" && (
                  <div className="group-empty" role="row">
                    <button className="link-button" onClick={() => setAdding(name)}>
                      <Plus size={13} /> {t("Add a category")}
                    </button>
                  </div>
                )}
              </div>
            );
          })}
          {!shown.length && (
            <div className="empty-state compact">
              {view === "all" ? (
                <>
                  <p>{t("Create a group, then categories for rent, groceries, saving, or anything you care about.")}</p>
                  <button className="secondary" onClick={() => setAddingGroup(true)}>
                    {t("Add a group")}
                  </button>
                </>
              ) : (
                <p>{t("No categories match this view this month.")}</p>
              )}
            </div>
          )}
          <div className="budget-total" role="row">
            <span className="row-gutter" />
            <span role="rowheader" className="category-cell">
              {t("Total")}
            </span>
            <span role="gridcell" className="num">
              {money(summary.assigned)}
            </span>
            <span role="gridcell" className="num">
              {money(monthActivity)}
            </span>
            <span role="gridcell" className="num">
              {money(monthAvailable)}
            </span>
            <span className="row-actions" />
          </div>
        </div>
      </div>
      <aside className={`inspector${selected.length ? " open" : ""}`} aria-label={t("Details")}>
        <BudgetInspector
          budget={budget}
          summary={summary}
          summaries={summaries}
          month={month}
          selected={selectedCategories}
          visibleIds={visibleIds}
          onAutoAssign={(mode, label, ids, all, change, asOfMonth) => void autoAssign(mode, label, ids, all, change, asOfMonth)}
          onEditCategory={onEditCategory}
          onAsk={onAsk}
          onPayCard={onPayCard}
          onClear={() => setSelected([])}
          edit={edit}
          targetOpen={!!targetFor && selected.length === 1 && selected[0] === targetFor}
          onTargetOpen={(open) => setTargetFor(open && selected.length === 1 ? selected[0] : null)}
          onSaved={undoToast}
        />
      </aside>
      {activity && (
        <ActivityPopover
          budget={budget}
          categoryId={activity.category.id}
          categoryName={activity.category.name}
          month={month}
          anchor={activity.anchor}
          onClose={() => setActivity(null)}
          onOpenRegister={() => {
            setActivity(null);
            onOpenRegister?.({ accountId: "all", category: activity.category.id, from: `${month}-01`, to: monthEnd(month) });
          }}
        />
      )}
      {moves && (
        <MovesPopover
          budget={budget}
          categoryId={moves.category.id}
          categoryName={moves.category.name}
          month={month}
          assigned={summaries.get(moves.category.id)?.assigned ?? 0}
          anchor={moves.anchor}
          onClose={() => setMoves(null)}
          onMoveFunds={() => {
            const c = moves.category;
            setMoves(null);
            // Moving funds starts from the category's Available, as it does in the row.
            const pill = document.querySelector<HTMLElement>(`[data-category="${c.id}"] .available`);
            if (pill) openMover(c, pill);
          }}
        />
      )}

      {menu && (
        <Menu open onClose={() => setMenu(null)} anchor={{ x: menu.x, y: menu.y }} items={menu.items} label={menu.label} />
      )}
      {mover && (
        <MoveMoneyPopover
          budget={budget}
          anchor={mover.anchor}
          source={mover.source}
          sourceName={mover.name}
          available={mover.available}
          balances={available}
          readyToAssign={rta}
          onMove={moveMoney}
          onClose={() => setMover(null)}
        />
      )}
      {deleting?.kind === "category" && (
        <DeleteCategory
          budget={budget}
          category={deleting.category}
          available={available}
          close={() => setDeleting(null)}
          confirm={(replacement) => {
            const c = deleting.category;
            edit((d) => deleteCategory(d, c.id, replacement));
            setSelected((s) => s.filter((id) => id !== c.id));
            setDeleting(null);
            undoToast(t("Deleted {name}.", { name: c.name }));
          }}
        />
      )}
      {deleting?.kind === "group" && (
        <DeleteGroup
          name={deleting.name}
          count={budget.categories.filter((c) => c.group === deleting.name).length}
          groups={realGroups.filter((g) => g !== deleting.name)}
          close={() => setDeleting(null)}
          confirm={(moveTo) => {
            const name = deleting.name;
            edit((d) => deleteGroup(d, name, moveTo));
            setDeleting(null);
            undoToast(moveTo ? t("Deleted {name}; its categories moved to {group}.", { name, group: moveTo }) : t("Deleted {name}.", { name }));
          }}
        />
      )}
    </div>
  );
}

function DeleteCategory({
  budget,
  category,
  available,
  close,
  confirm,
}: {
  budget: Budget;
  category: Category;
  available: Map<string, number>;
  close: () => void;
  confirm: (replacement: string | null) => void;
}) {
  const [target, setTarget] = useState<string | null>(READY_TO_ASSIGN);
  const used =
    budget.transactions.some((t) => t.splits.some((s) => s.categoryId === category.id)) ||
    budget.assignments.some((a) => a.categoryId === category.id);
  return (
    <Modal title={t("Delete {name}?", { name: category.name })} close={close}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          confirm(target === READY_TO_ASSIGN ? null : target);
        }}
      >
        {used ? (
          <label>
            {t("Move its transactions, scheduled transactions and assigned money to")}
            <CategoryPicker
              budget={budget}
              value={target}
              onChange={setTarget}
              available={available}
              exclude={category.id}
              readyToAssign={t("Unassigned (release its money)")}
              includeHidden
              label={t("Move history to")}
            />
          </label>
        ) : (
          <p>{t("It has no transactions or assigned money.")}</p>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="danger" disabled={!target}>
            {t("Delete category")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function DeleteGroup({
  name,
  count,
  groups,
  close,
  confirm,
}: {
  name: string;
  count: number;
  groups: string[];
  close: () => void;
  confirm: (moveTo?: string) => void;
}) {
  const [moveTo, setMoveTo] = useState(groups[0] ?? "");
  return (
    <Modal title={t("Delete {name}?", { name })} close={close}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          confirm(count ? moveTo : undefined);
        }}
      >
        {count ? (
          groups.length ? (
            <label>
              {tn(count, "Move its {count} category to", "Move its {count} categories to")}
              <select value={moveTo} onChange={(e) => setMoveTo(e.target.value)}>
                {groups.map((g) => (
                  <option key={g}>{g}</option>
                ))}
              </select>
            </label>
          ) : (
            <p>{t("This is the only group. Create another group first, or delete its categories.")}</p>
          )
        ) : (
          <p>{t("The group is empty.")}</p>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="danger" disabled={!!count && !groups.length}>
            {t("Delete group")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
