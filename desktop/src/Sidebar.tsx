import { useEffect, useMemo, useRef, useState, type DragEvent, type ReactNode } from "react";
import { Archive, ArchiveRestore, ArrowDown, ArrowLeftRight, ArrowUp, ChartColumn, Check, CheckCheck, ChevronDown, ChevronRight, ChevronsUpDown, Download, FileUp, FolderOpen, Info, Landmark, LayoutGrid, MoreHorizontal, PanelLeftClose, PanelLeftOpen, Pencil, Plus, RefreshCw, Settings2, Tags } from "lucide-react";
import { Menu, startEdge, useContextMenu, useToast, type MenuItem } from "./ui";
import { accountType, inNetWorth, typeName } from "./accounts";
import { uncategorizedCounts } from "./uncategorized";
import { api, type BudgetListing } from "./bridge";
import { formatDate, money, tone } from "./format";
import { staleSync } from "./BankSync";
import { hasLoan } from "./Planning";
import { keyLabel } from "./platform";
import { getDirection, t, tn } from "./i18n";
import type { Account, Budget, Summary } from "./types";

export type Page =
  | "budget"
  | "transactions"
  | "settings"
  | "loans"
  | "reports"
  | "payees"
  | "import";
type Edit = (update: (draft: Budget) => void) => void;

const WIDTH = { min: 200, max: 360, default: 248, collapsed: 52 };
const store = {
  get: (key: string) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set: (key: string, value: string) => {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* not remembered */
    }
  },
};

/** Sidebar width and collapsed state, remembered between sessions. Ctrl+B toggles. */
export function useSidebarLayout() {
  const [width, setWidthState] = useState(() => {
    const saved = Number(store.get("lb:sidebar:width"));
    return saved >= WIDTH.min && saved <= WIDTH.max ? saved : WIDTH.default;
  });
  const [collapsed, setCollapsed] = useState(() => store.get("lb:sidebar:collapsed") === "1");
  const setWidth = (value: number) => {
    const next = Math.round(Math.min(WIDTH.max, Math.max(WIDTH.min, value)));
    setWidthState(next);
    store.set("lb:sidebar:width", String(next));
  };
  const toggle = () =>
    setCollapsed((c) => {
      store.set("lb:sidebar:collapsed", c ? "0" : "1");
      return !c;
    });
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  return { width: collapsed ? WIDTH.collapsed : width, expandedWidth: width, collapsed, setWidth, toggle };
}

function ResizeHandle({ width, setWidth }: { width: number; setWidth: (w: number) => void }) {
  return (
    <div
      className="sidebar-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label={t("Resize sidebar")}
      aria-valuemin={WIDTH.min}
      aria-valuemax={WIDTH.max}
      aria-valuenow={width}
      tabIndex={0}
      title={t("Drag to resize · double-click to reset")}
      onPointerDown={(e) => {
        e.preventDefault();
        const handle = e.currentTarget;
        handle.setPointerCapture(e.pointerId);
        // The sidebar is on the right, right to left.
        const move = (m: PointerEvent) =>
          setWidth(getDirection() === "rtl" ? document.documentElement.clientWidth - m.clientX : m.clientX);
        const up = () => {
          handle.removeEventListener("pointermove", move);
          handle.removeEventListener("pointerup", up);
        };
        handle.addEventListener("pointermove", move);
        handle.addEventListener("pointerup", up);
      }}
      onDoubleClick={() => setWidth(WIDTH.default)}
      onKeyDown={(e) => {
        const wider = getDirection() === "rtl" ? "ArrowLeft" : "ArrowRight";
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") setWidth(width + (e.key === wider ? 16 : -16));
      }}
    />
  );
}

function BudgetSwitcher({
  budget,
  collapsed,
  onSwitch,
  onNew,
  onManage,
}: {
  budget: Budget;
  collapsed: boolean;
  onSwitch: (id: string) => void;
  onNew: () => void;
  onManage: () => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [budgets, setBudgets] = useState<BudgetListing[]>([]);
  const items: MenuItem[] = [
    ...budgets.map((b) => ({
      label: b.name,
      icon: b.current ? <Check size={14} /> : undefined,
      hint: b.currency,
      disabled: b.missing,
      onSelect: () => !b.current && onSwitch(b.id),
    })),
    "separator" as const,
    { label: t("New budget…"), icon: <Plus size={14} />, onSelect: onNew },
    { label: t("Manage budgets…"), icon: <FolderOpen size={14} />, onSelect: onManage },
  ];
  return (
    <>
      <button
        ref={button}
        className="budget-switcher"
        aria-label={t("Budget: {name}. Switch budget", { name: budget.name })}
        aria-expanded={open}
        title={collapsed ? budget.name : t("Switch budget")}
        onClick={() => {
          void api.listBudgets().then(rows => setBudgets(rows.filter(row => !["trashed", "purged"].includes(row.lifecycle ?? "active")))).catch(() => setBudgets([]));
          setOpen(!open);
        }}
      >
        <span className="brand-mark">{budget.name.trim().slice(0, 2).toUpperCase() || "LB"}</span>
        {!collapsed && (
          <>
            <span className="budget-switcher-text">
              <strong>{budget.name}</strong>
            </span>
            <ChevronsUpDown size={14} className="muted" />
          </>
        )}
      </button>
      <Menu open={open} onClose={() => setOpen(false)} anchor={button} items={items} label={t("Budgets")} />
    </>
  );
}

function NavItem({
  active,
  icon,
  children,
  count,
  collapsed,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  children: string;
  count?: number;
  collapsed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      className={`nav-item${active ? " active" : ""}`}
      onClick={onClick}
      title={collapsed ? children : undefined}
      data-tooltip-side="right"
      aria-label={collapsed ? children : undefined}
      aria-current={active ? "page" : undefined}
    >
      {icon}
      {!collapsed && <span>{children}</span>}
      {!collapsed && count !== undefined && <small>{count.toLocaleString("en-GB")}</small>}
    </button>
  );
}

type Section = { key: "budget" | "tracking" | "closed"; name: string; accounts: Account[] };

export function Sidebar({
  budget,
  budgetId,
  balances,
  page,
  accountId,
  layout,
  edit,
  onNavigate,
  onAccount,
  onAddAccount,
  onEditAccount,
  onReconcile,
  onExportAccount,
  onSync,
  onSyncDetails,
  onLoan,
  onSwitchBudget,
  onNewBudget,
  onManageBudgets,
  importing,
}: {
  budget: Budget;
  budgetId: string;
  balances: Map<string, Summary["accounts"][number]>;
  page: Page;
  accountId: string;
  layout: ReturnType<typeof useSidebarLayout>;
  edit: Edit;
  onNavigate: (page: Page) => void;
  onAccount: (id: string) => void;
  onAddAccount: () => void;
  onEditAccount: (a: Account) => void;
  onReconcile: (a: Account) => void;
  /** Saves the account's transactions as a CSV file. */
  onExportAccount?: (a: Account) => void;
  /** Syncs an account linked to its bank. */
  onSync?: (a: Account) => void;
  /** Shows how a linked account syncs. */
  onSyncDetails?: (a: Account) => void;
  /** A debt's loan details and payoff planner (LoanHeader.tsx). */
  onLoan?: (a: Account) => void;
  onSwitchBudget: (id: string) => void;
  onNewBudget: () => void;
  /** An import in progress, by the account it's into: a way back to it. */
  importing?: string;
  onManageBudgets: () => void;
}) {
  const toast = useToast();
  const { collapsed } = layout;
  const sectionKey = `lb:sidebar:sections:${budgetId}`;
  const [closedSections, setClosedSections] = useState<Set<string>>(
    () => new Set(JSON.parse(store.get(sectionKey) ?? '["closed"]') as string[]),
  );
  useEffect(() => {
    setClosedSections(new Set(JSON.parse(store.get(sectionKey) ?? '["closed"]') as string[]));
  }, [sectionKey]);
  const toggleSection = (key: string) =>
    setClosedSections((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      store.set(sectionKey, JSON.stringify([...next]));
      return next;
    });
  const approvals = useMemo(() => {
    const counts = new Map<string, number>();
    for (const t of budget.transactions)
      if (t.approved === false) counts.set(t.accountId, (counts.get(t.accountId) ?? 0) + 1);
    return counts;
  }, [budget.transactions]);
  // Spending that needs a category, by the account it spends (uncategorized.ts).
  const uncategorized = useMemo(
    () => uncategorizedCounts(budget).byAccount,
    [budget.transactions, budget.accounts, budget.categories],
  );
  const sections: Section[] = [
    { key: "budget", name: t("Budget"), accounts: budget.accounts.filter((a) => !a.archived && a.kind !== "tracking") },
    { key: "tracking", name: t("Tracking"), accounts: budget.accounts.filter((a) => !a.archived && a.kind === "tracking") },
    { key: "closed", name: t("Closed"), accounts: budget.accounts.filter((a) => a.archived) },
  ];

  const [drag, setDrag] = useState<{ id: string; section: string } | null>(null);
  const [drop, setDrop] = useState<{ id: string; after: boolean } | null>(null);
  const move = (id: string, target: string, after: boolean) => {
    if (id === target) return;
    edit((d) => {
      const moving = d.accounts.find((a) => a.id === id)!;
      const rest = d.accounts.filter((a) => a.id !== id);
      const index = rest.findIndex((a) => a.id === target) + (after ? 1 : 0);
      rest.splice(index, 0, moving);
      d.accounts = rest;
    });
  };
  const step = (a: Account, list: Account[], delta: number) => {
    const neighbour = list[list.indexOf(a) + delta];
    if (neighbour) move(a.id, neighbour.id, delta > 0);
  };
  const setClosed = (a: Account, archived: boolean) => {
    edit((d) => {
      d.accounts.find((x) => x.id === a.id)!.archived = archived;
    });
    const balance = balances.get(a.id)?.balance ?? 0;
    toast({
      message: archived
        ? balance
          ? t("Closed {account}. It still has a balance of {amount}.", { account: a.name, amount: money(balance, { symbol: true }) })
          : t("Closed {account}.", { account: a.name })
        : t("Reopened {account}.", { account: a.name }),
      action: {
        label: t("Undo"),
        onClick: () =>
          edit((d) => {
            d.accounts.find((x) => x.id === a.id)!.archived = !archived;
          }),
      },
    });
  };
  const menu = useContextMenu();
  const [menuFor, setMenuFor] = useState<{ account: Account; list: Account[] } | null>(null);
  const menuItems = (a: Account, list: Account[]): MenuItem[] => [
    { label: t("Edit account…"), icon: <Pencil size={14} />, onSelect: () => onEditAccount(a) },
    { label: t("Reconcile…"), icon: <CheckCheck size={14} />, disabled: a.archived, onSelect: () => onReconcile(a) },
    ...(onLoan && hasLoan(budget, a, balances.get(a.id)?.balance)
      ? [{ label: t("Loan details & payoff…"), icon: <Landmark size={14} />, onSelect: () => onLoan(a) }]
      : []),
    ...(onSync && a.simplefin && !a.archived
      ? [{ label: t("Sync with bank"), icon: <RefreshCw size={14} />, onSelect: () => onSync(a) }]
      : []),
    ...(onSyncDetails && a.simplefin
      ? [{ label: t("Bank sync details…"), icon: <Info size={14} />, onSelect: () => onSyncDetails(a) }]
      : []),
    ...(onExportAccount
      ? [{ label: t("Export transactions"), icon: <Download size={14} />, onSelect: () => onExportAccount(a) }]
      : []),
    "separator",
    { label: t("Move up"), icon: <ArrowUp size={14} />, disabled: list[0] === a, onSelect: () => step(a, list, -1) },
    { label: t("Move down"), icon: <ArrowDown size={14} />, disabled: list.at(-1) === a, onSelect: () => step(a, list, 1) },
    "separator",
    a.archived
      ? { label: t("Reopen account"), icon: <ArchiveRestore size={14} />, onSelect: () => setClosed(a, false) }
      : { label: t("Close account"), icon: <Archive size={14} />, onSelect: () => setClosed(a, true) },
  ];

  const dragProps = (a: Account, section: Section) => ({
    draggable: true,
    onDragStart: (e: DragEvent) => {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", a.id);
      setDrag({ id: a.id, section: section.key });
    },
    onDragOver: (e: DragEvent) => {
      if (!drag || drag.section !== section.key || drag.id === a.id) return;
      e.preventDefault();
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      setDrop({ id: a.id, after: e.clientY > rect.top + rect.height / 2 });
    },
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      if (drag && drop) move(drag.id, drop.id, drop.after);
      setDrag(null);
      setDrop(null);
    },
    onDragEnd: () => {
      setDrag(null);
      setDrop(null);
    },
  });

  const collapseButton = (
    <button
      className="icon-button collapse-button"
      aria-label={collapsed ? t("Expand sidebar") : t("Collapse sidebar")}
      title={keyLabel(collapsed ? t("Expand sidebar (Ctrl+B)") : t("Collapse sidebar (Ctrl+B)"))}
      onClick={layout.toggle}
    >
      {collapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
    </button>
  );

  return (
    <aside className={`sidebar${collapsed ? " collapsed" : ""}`}>
      <div className="sidebar-top">
        <BudgetSwitcher
          budget={budget}
          collapsed={collapsed}
          onSwitch={onSwitchBudget}
          onNew={onNewBudget}
          onManage={onManageBudgets}
        />
        {!collapsed && collapseButton}
      </div>
      {/* Collapsed, the band has room for the budget mark only. */}
      {collapsed && collapseButton}
      <div className="sidebar-scroll">
        <nav aria-label={t("Main navigation")}>
          <NavItem collapsed={collapsed} active={page === "budget"} icon={<LayoutGrid size={16} strokeWidth={1.75} />} onClick={() => onNavigate("budget")}>
            {t("Budget")}
          </NavItem>
          <NavItem collapsed={collapsed} active={page === "reports"} icon={<ChartColumn size={16} strokeWidth={1.75} />} onClick={() => onNavigate("reports")}>
            {t("Reports")}
          </NavItem>
          <NavItem collapsed={collapsed} active={page === "payees"} icon={<Tags size={16} strokeWidth={1.75} />} onClick={() => onNavigate("payees")}>
            {t("Payees & rules")}
          </NavItem>
          {importing !== undefined && (
            <NavItem collapsed={collapsed} active={page === "import"} icon={<FileUp size={16} strokeWidth={1.75} />} onClick={() => onNavigate("import")}>
              {importing ? t("Importing into {account}", { account: importing }) : t("Importing a file")}
            </NavItem>
          )}
          {/* All transactions sits last, just above the accounts it covers. */}
          <NavItem
            collapsed={collapsed}
            active={page === "transactions" && accountId === "all"}
            icon={<ArrowLeftRight size={16} strokeWidth={1.75} />}
            count={budget.transactions.length}
            onClick={() => onAccount("all")}
          >
            {t("All transactions")}
          </NavItem>
        </nav>
        {!collapsed && (
          <div className="accounts">
            <div className="accounts-heading">
              <span>{t("Accounts")}</span>
              <button className="icon-button" title={t("Add account")} aria-label={t("Add account")} onClick={onAddAccount}>
                <Plus size={15} />
              </button>
            </div>
            {sections
              .filter((s) => s.accounts.length)
              .map((section) => {
                const open = !closedSections.has(section.key);
                // Accounts left out of net worth are left out of the section total too.
                const total = section.accounts.reduce(
                  (n, a) => n + (inNetWorth(a) ? (balances.get(a.id)?.balance ?? 0) : 0),
                  0,
                );
                return (
                  <div key={section.key} className="account-section">
                    <button
                      className="account-section-title"
                      aria-expanded={open}
                      onClick={() => toggleSection(section.key)}
                    >
                      {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                      <span>{section.name}</span>
                      <span className={`num ${tone(total)}`}>{money(total)}</span>
                    </button>
                    {open &&
                      section.accounts.map((a) => {
                        const balance = balances.get(a.id)?.balance ?? 0;
                        const pending = approvals.get(a.id);
                        const needs = uncategorized.get(a.id);
                        const current = page === "transactions" && accountId === a.id;
                        const link = a.simplefin;
                        const synced = link
                          ? `${
                              link.name != null
                                ? link.org
                                  ? t("Syncs with {name} at {bank}", { name: link.name, bank: link.org })
                                  : t("Syncs with {name}", { name: link.name })
                                : link.org
                                  ? t("Syncs with the bank at {bank}", { bank: link.org })
                                  : t("Syncs with the bank")
                            } · ${link.lastSync ? t("last synced {date}", { date: formatDate(link.lastSync) }) : t("not synced yet")}`
                          : "";
                        return (
                          <div
                            key={a.id}
                            className={`account-row${drop?.id === a.id ? (drop.after ? " drop-after" : " drop-before") : ""}${drag?.id === a.id ? " dragging" : ""}`}
                            {...dragProps(a, section)}
                            onContextMenu={(e) => {
                              setMenuFor({ account: a, list: section.accounts });
                              menu.open(e);
                            }}
                          >
                            <button
                              className={`account-item${current ? " active" : ""}${inNetWorth(a) ? "" : " outside"}`}
                              aria-current={current ? "page" : undefined}
                              onClick={() => onAccount(a.id)}
                              title={[
                                a.name,
                                typeName(accountType(a, balance)),
                                ...(inNetWorth(a) ? [] : [t("not counted in net worth")]),
                                ...(link ? [t("syncs with its bank")] : []),
                              ].join(" · ")}
                            >
                              <span className="account-name">
                                <span>{a.name}</span>
                                {link && (
                                  // A mark, not a second button: the row stays one control, and the
                                  // "…" menu has Sync and the details for the keyboard.
                                  <span
                                    className={`sync-mark${staleSync(a) ? " stale" : ""}`}
                                    title={t("{status}. Click for details.", { status: synced })}
                                    onClick={
                                      onSyncDetails
                                        ? (e) => {
                                            e.stopPropagation();
                                            onSyncDetails(a);
                                          }
                                        : undefined
                                    }
                                  >
                                    <RefreshCw size={10} aria-hidden />
                                  </span>
                                )}
                              </span>
                              {!!needs && (
                                <span className="needs-mark" title={tn(needs, "{count} needs a category", "{count} need a category")}>
                                  <span className="sr-only">{tn(needs, ", {count} needs a category", ", {count} need a category")}</span>
                                </span>
                              )}
                              {!!pending && (
                                <span className="badge" title={tn(pending, "{count} to approve", "{count} to approve")}>
                                  {pending}
                                </span>
                              )}
                              <span className={`num ${tone(balance)}`}>{money(balance)}</span>
                            </button>
                            <button
                              className="icon-button account-more"
                              aria-label={t("Actions for {name}", { name: a.name })}
                              onClick={(e) => {
                                const rect = e.currentTarget.getBoundingClientRect();
                                setMenuFor({ account: a, list: section.accounts });
                                menu.openAt(startEdge(rect), rect.bottom);
                              }}
                            >
                              <MoreHorizontal size={14} />
                            </button>
                          </div>
                        );
                      })}
                  </div>
                );
              })}
            {!budget.accounts.length && (
              <p className="sidebar-hint">{t("Add an account to give your money a home.")}</p>
            )}
          </div>
        )}
      </div>
      <div className="sidebar-bottom">
        <NavItem collapsed={collapsed} active={page === "settings"} icon={<Settings2 size={16} strokeWidth={1.75} />} onClick={() => onNavigate("settings")}>
          {t("Settings & data")}
        </NavItem>
      </div>
      {!collapsed && <ResizeHandle width={layout.expandedWidth} setWidth={layout.setWidth} />}
      {menuFor && menu.at && (
        <Menu
          open
          onClose={() => {
            menu.close();
            setMenuFor(null);
          }}
          anchor={menu.at}
          items={menuItems(menuFor.account, menuFor.list)}
          label={t("Actions for {name}", { name: menuFor.account.name })}
        />
      )}
    </aside>
  );
}
