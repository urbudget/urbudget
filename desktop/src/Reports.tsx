import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  ChartPie,
  ChartSpline,
  ChevronDown,
  Download,
  EyeOff,
  Landmark,
  LoaderCircle,
  Scale,
  SlidersHorizontal,
  Telescope,
  TrendingUp,
  X,
  type LucideIcon,
} from "lucide-react";
import { api, read, type ReportFilter } from "./bridge";
import { errorText, money, shiftMonth } from "./format";
import { UNCATEGORIZED_CATEGORY } from "./accounting";
import { NEEDS_CATEGORY } from "./uncategorized";
import { groupNames } from "./categories";
import {
  DateRangePicker,
  HeaderSlot,
  Popover,
  Tabs,
  describeRange,
  rangeFor,
  useToast,
  type DateRange,
} from "./ui";
import { today, type Budget } from "./types";
import type { RegisterFilter } from "./Register";
import { OTHER, SERIES, firstDay, fmt, lastDay, type Figure, type ReportData, type Scope } from "./reports/shared";
import { TransactionsDialog, type LineQuery } from "./reports/TransactionsDialog";
import { ByCategory, ByPayee, OverTime, SpendingCalendar } from "./reports/Spending";
import { CashFlow, IncomeBreakdown, IncomeExpense } from "./reports/Income";
import { AgeOfMoney, BalanceSheet, Balances, NetWorth } from "./reports/Wealth";
import { Forecast } from "./reports/Forecast";
import { Investing, type InvestmentsReport } from "./reports/Investing";
import { Overview } from "./reports/Overview";
import { holdingAccounts } from "./reports/portfolio";
import { chosen, useHidden } from "./reports/filters";
import { msg, t, tn, tnc, useLanguage } from "./i18n";
import "./reports.css";

// Six reports, each answering one question. A report's views are the report bodies that answer
// it in different ways; the switch under its title chooses among them.

/** A report body: one of a report's views. */
type ViewId =
  | "categories"
  | "payees"
  | "trends"
  | "calendar"
  | "incomeExpense"
  | "sources"
  | "cashflow"
  | "networth"
  | "balances"
  | "balance"
  | "performance"
  | "allocation"
  | "earnings"
  | "forecast"
  | "age";
export type ReportKey = "spending" | "income" | "networth" | "balance" | "investments" | "forecast";
/** Where Reports is: on the overview, or in a report. */
type Place = "overview" | ReportKey;
type ViewInfo = {
  id: ViewId;
  /** Its name on the report's switch. */
  label: string;
  blurb: string;
  /** The Categories filter applies. */
  categories?: boolean;
  /** It reads its own figures, so the shared report isn't asked for. */
  own?: boolean;
  /** It has no months to choose: it looks ahead from today, or at today alone. */
  dateless?: boolean;
};
type ReportInfo = {
  id: ReportKey;
  /** The report's name: its tab, its card and its page heading. */
  label: string;
  /** The question it answers, under its name. */
  question: string;
  icon: LucideIcon;
  /** The first is the one it opens on. */
  views: ViewInfo[];
};
// Names, questions and blurbs are translated when shown.
const REPORTS: ReportInfo[] = [
  {
    id: "spending",
    label: msg("Spending"),
    question: msg("Where did my money go?"),
    icon: ChartPie,
    views: [
      {
        id: "categories",
        label: msg("Category"),
        blurb: msg("Where your money went, by group and category, and how last month compares."),
        categories: true,
      },
      { id: "payees", label: msg("Payee"), blurb: msg("Who you pay most, and how often."), categories: true },
      { id: "trends", label: msg("Over time"), blurb: msg("Spending month by month, by group or in total."), categories: true },
      {
        id: "calendar",
        label: msg("Calendar"),
        blurb: msg("Spending day by day: busy days, quiet days and weekdays."),
        categories: true,
      },
    ],
  },
  {
    id: "income",
    label: msg("Income"),
    question: msg("Am I spending less than I earn?"),
    icon: Scale,
    views: [
      {
        id: "incomeExpense",
        label: msg("Vs. spending"),
        blurb: msg("Income, spending and the surplus left in your budget."),
        categories: true,
      },
      { id: "sources", label: msg("Sources"), blurb: msg("Where your income comes from, and how steady it is.") },
      {
        id: "cashflow",
        label: msg("In and out"),
        blurb: msg("Account movements across your budget boundary, including credit purchases."),
        categories: true,
      },
    ],
  },
  {
    id: "networth",
    label: msg("Net worth"),
    question: msg("What am I worth, and why did it change?"),
    icon: TrendingUp,
    views: [
      { id: "networth", label: msg("Over time"), blurb: msg("What you own minus what you owe, month by month.") },
      { id: "balances", label: msg("By account"), blurb: msg("Each account's balance at the end of every month.") },
    ],
  },
  {
    id: "balance",
    label: msg("Balance sheet"),
    question: msg("What do I own and owe?"),
    icon: Landmark,
    views: [{ id: "balance", label: msg("Balance sheet"), blurb: msg("Assets and liabilities by type, with the ratios that matter.") }],
  },
  {
    id: "investments",
    label: msg("Investments"),
    question: msg("How are my investments doing?"),
    icon: ChartSpline,
    views: [
      {
        id: "performance",
        label: msg("Performance"),
        blurb: msg("How your investments did: their value, what you put in, and what they gained."),
        own: true,
      },
      {
        id: "allocation",
        label: msg("Allocation"),
        blurb: msg("What your investments are in today, across all your accounts."),
        own: true,
        dateless: true,
      },
      {
        id: "earnings",
        label: msg("Income and costs"),
        blurb: msg("Dividends and interest earned, and the tax and fees paid."),
        own: true,
      },
    ],
  },
  {
    id: "forecast",
    label: msg("Forecast"),
    question: msg("What's coming up?"),
    icon: Telescope,
    views: [
      {
        id: "forecast",
        label: msg("Balances"),
        blurb: msg("Where your cash and card balances are heading, from what's scheduled."),
        own: true,
        dateless: true,
      },
      { id: "age", label: msg("Cash age"), blurb: msg("How long money waits in your accounts before you spend it.") },
    ],
  },
];
/** Where the person was in Reports, as a view's id or "overview", for as long as the app runs. */
const PLACE_KEY = "lb:reports:place";
/** The view last used in each report. */
const VIEWS_KEY = "lb:reports:views";
// Before reports had views, each view was a report of its own: the last one opened and the last
// in each group were remembered by their ids, which are the views' ids but for the income sources.
const OLD_VIEW_KEY = "lb:reports:view",
  OLD_GROUP_VIEWS_KEY = "lb:reports:groupViews";
const OLD_IDS: Record<string, ViewId> = { income: "sources" };
type Views = Partial<Record<ReportKey, ViewId>>;

/**
 * The report an id names, and the view when it names one: a report's own id, a view's, or with
 * `old` the id of a report from before reports had views.
 */
export function placeOf(id: unknown, old = false): { report: ReportKey; view?: ViewId } | null {
  if (typeof id !== "string") return null;
  const report = old ? undefined : REPORTS.find((r) => r.id === id);
  if (report) return { report: report.id };
  const view = (old && OLD_IDS[id]) || id;
  const owner = REPORTS.find((r) => r.views.some((v) => v.id === view));
  return owner ? { report: owner.id, view: view as ViewId } : null;
}

/** The remembered view of each report; the first time, from what was remembered before reports had views. */
function loadViews(): Views {
  const views: Views = {};
  const keep = (ids: unknown[], old: boolean) => {
    for (const id of ids) {
      const at = placeOf(id, old);
      if (at?.view) views[at.report] = at.view;
    }
  };
  try {
    const saved = localStorage.getItem(VIEWS_KEY);
    if (saved) keep(Object.values(JSON.parse(saved) ?? {}), false);
    else {
      keep([...Object.values(JSON.parse(localStorage.getItem(OLD_GROUP_VIEWS_KEY) ?? "{}") ?? {}), localStorage.getItem(OLD_VIEW_KEY)], true);
      localStorage.setItem(VIEWS_KEY, JSON.stringify(views));
      localStorage.removeItem(OLD_VIEW_KEY);
      localStorage.removeItem(OLD_GROUP_VIEWS_KEY);
    }
  } catch {
    // No storage, or nothing that reads: every report opens on its first view.
  }
  return views;
}

type FilterGroup = { name: string; items: { id: string; label: string }[] };

/**
 * The one control for what the reports cover. Its label says what is in force ("Last 12 months ·
 * All accounts"), and it opens the range and the account and category filters to change them.
 */
function Filters({ label, narrowed, children }: { label: string; narrowed: boolean; children: ReactNode }) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        ref={button}
        className={`secondary filter-button report-filters${narrowed ? " active" : ""}`}
        aria-expanded={open}
        aria-label={`${t("Filters")}: ${label}`}
        onClick={() => setOpen(!open)}
      >
        <SlidersHorizontal size={15} aria-hidden /> <span className="filter-summary">{label}</span> <ChevronDown size={14} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={t("Filters")} className="report-filter-panel">
        {children}
      </Popover>
    </>
  );
}
/** What a filter shows, in a few words: all of it, the one thing, or how many. */
function shown(groups: FilterGroup[], hidden: Set<string>, all: string, count: (n: number) => string) {
  const items = groups.flatMap((g) => g.items);
  const on = items.filter((i) => !hidden.has(i.id));
  return on.length === items.length ? all : on.length === 1 ? on[0].label : count(on.length);
}

/**
 * A filter button with a searchable, grouped checklist of what reports show. It changes what is
 * hidden; hidden ids it doesn't list (tracking accounts in the forecast) stay hidden.
 */
function MultiFilter({
  label,
  allLabel,
  searchLabel,
  countLabel,
  hiddenLabel,
  groups,
  hidden,
  onChange,
}: {
  label: string;
  allLabel: string;
  /** The search field's placeholder, such as "Search accounts". */
  searchLabel: string;
  /** How several chosen items read, such as "3 accounts". */
  countLabel: (count: number) => string;
  /** How a few hidden items read, such as "2 hidden". */
  hiddenLabel: (count: number) => string;
  groups: FilterGroup[];
  hidden: Set<string>;
  onChange: (hidden: Set<string>) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const all = groups.flatMap((g) => g.items);
  const on = all.filter((i) => !hidden.has(i.id));
  const off = all.length - on.length;
  const shown = groups
    .map((g) => ({ ...g, items: g.items.filter((i) => i.label.toLowerCase().includes(query.toLowerCase())) }))
    .filter((g) => g.items.length);
  const summary = !off
    ? allLabel
    : !on.length
      ? t("None")
      : on.length === 1
        ? on[0].label
        : off <= on.length
          ? hiddenLabel(off)
          : countLabel(on.length);
  const show = (ids: string[], visible: boolean) => {
    const next = new Set(hidden);
    for (const id of ids) {
      if (visible) next.delete(id);
      else next.add(id);
    }
    onChange(next);
  };
  const isOn = (id: string) => !hidden.has(id);
  return (
    <>
      <button
        ref={button}
        className={`secondary filter-button${off ? " active" : ""}`}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <span className="muted">{label}:</span> <span className="filter-summary">{summary}</span> <ChevronDown size={14} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={label} className="filter-popover">
        <div className="filter-popover-head">
          <input
            placeholder={searchLabel}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button className="link-button" onClick={() => show(all.map((i) => i.id), true)}>
            {t("All")}
          </button>
          <button className="link-button" onClick={() => show(all.map((i) => i.id), false)}>
            {t("None")}
          </button>
        </div>
        <div className="filter-popover-list">
          {shown.map((g) => {
            const ids = g.items.map((i) => i.id);
            const every = ids.every(isOn);
            return (
              <fieldset key={g.name}>
                <label className="checkbox-label group">
                  <input type="checkbox" checked={every} onChange={() => show(ids, !every)} />
                  {g.name}
                </label>
                {g.items.map((i) => (
                  <label key={i.id} className="checkbox-label">
                    <input type="checkbox" checked={isOn(i.id)} onChange={(e) => show([i.id], e.target.checked)} />
                    {i.label}
                  </label>
                ))}
              </fieldset>
            );
          })}
        </div>
        <p className="filter-popover-foot">{t("What you clear here stays left out of reports for this budget, on this computer.")}</p>
      </Popover>
    </>
  );
}

/** At most this many names in a "left out" line; the rest are counted. */
const NAMED = 6;

/**
 * Says what a report leaves out, right under the filters, so a total is never read as everything:
 * the hidden items, each with a way back in, or the few still shown when most are hidden.
 */
function LeftOut({
  kind,
  groups,
  hidden,
  onChange,
  note,
}: {
  kind: "accounts" | "categories";
  groups: FilterGroup[];
  hidden: Set<string>;
  onChange: (hidden: Set<string>) => void;
  /** Something else the figures leave out, said in words: it can't be put back. */
  note?: ReactNode;
}) {
  const all = groups.flatMap((g) => g.items);
  const off = all.filter((i) => hidden.has(i.id));
  if (!off.length)
    return note ? (
      <div className="report-left-out">
        <EyeOff size={14} aria-hidden />
        <span className="report-left-out-note">{note}</span>
      </div>
    ) : null;
  const on = all.filter((i) => !hidden.has(i.id));
  const showAll = () => onChange(new Set([...hidden].filter((id) => !all.some((i) => i.id === id))));
  // Name the smaller side: what is left out, or the few still shown.
  const only = off.length > on.length;
  const named = (only ? on : off).slice(0, NAMED);
  const more = (only ? on : off).length - named.length;
  const accounts = kind === "accounts";
  return (
    <div className="report-left-out">
      <EyeOff size={14} aria-hidden />
      <span>
        {!on.length
          ? accounts ? t("No accounts are shown.") : t("No categories are shown.")
          : only
            ? accounts ? t("Only these accounts:") : t("Only these categories:")
            : accounts ? t("Accounts left out:") : t("Categories left out:")}
      </span>
      {named.map((i) =>
        only ? (
          <span key={i.id} className="chip chip-text" title={i.label}>
            <span className="chip-label">{i.label}</span>
          </span>
        ) : (
          <span key={i.id} className="chip" title={i.label}>
            <span className="chip-label">{i.label}</span>
            <button
              className="chip-remove"
              aria-label={t("Show {name} in reports", { name: i.label })}
              title={t("Show {name} in reports", { name: i.label })}
              onClick={() => onChange(new Set([...hidden].filter((id) => id !== i.id)))}
            >
              <X size={12} aria-hidden />
            </button>
          </span>
        ),
      )}
      {more > 0 && <span className="muted">{tn(more, "+{count} more", "+{count} more")}</span>}
      <button className="link-button" onClick={showAll}>
        {accounts ? t("Show all accounts") : t("Show all categories")}
      </button>
      {note && <span className="report-left-out-note">{note}</span>}
    </div>
  );
}

export function Reports({
  budget,
  budgetId,
  month,
  onInspect,
  onOpenAccount,
}: {
  budget: Budget;
  /** Keys what the reports leave out, which each budget remembers. */
  budgetId: string;
  month: string;
  /** Opens the register on these transactions. */
  onInspect: (filter: RegisterFilter) => void;
  /** Opens an account's own page: its Holdings view, for one with holdings. */
  onOpenAccount?: (accountId: string) => void;
}) {
  const toast = useToast();
  const language = useLanguage();
  const earliest = useMemo(
    () => budget.transactions.reduce((m, t) => (t.date < m ? t.date : m), today()).slice(0, 7),
    [budget.transactions],
  );
  const [range, setRange] = useState<DateRange>(() =>
    month === today().slice(0, 7)
      ? rangeFor("last12", "month", new Date(), earliest)
      : { preset: "custom", from: shiftMonth(month, -11), to: month },
  );
  const [hidden, setHidden] = useHidden(budgetId);
  // The Investments report is there once an account has holdings.
  const held = useMemo(() => new Set(holdingAccounts(budget).map((a) => a.id)), [budget.accounts, budget.investments]);
  const reports = held.size ? REPORTS : REPORTS.filter((r) => r.id !== "investments");
  // Reports opens on the overview when the app starts, and afterwards where the person was.
  const [at, setAt] = useState<{ place: Place; views: Views }>(() => {
    const views = loadViews();
    try {
      const was = placeOf(sessionStorage.getItem(PLACE_KEY));
      if (was) return { place: was.report, views: was.view ? { ...views, [was.report]: was.view } : views };
    } catch {
      // No storage: the overview.
    }
    return { place: "overview", views };
  });
  // A report remembered from a budget with holdings isn't there in one without.
  const report = reports.find((r) => r.id === at.place) ?? null;
  const view = report && (report.views.find((v) => v.id === at.views[report.id]) ?? report.views[0]);
  const go = (place: Place, to?: ViewId) => {
    const views = to && place !== "overview" ? { ...at.views, [place]: to } : at.views;
    setAt({ place, views });
    try {
      const opened = REPORTS.find((r) => r.id === place);
      sessionStorage.setItem(PLACE_KEY, opened ? (views[opened.id] ?? opened.views[0].id) : "overview");
      localStorage.setItem(VIEWS_KEY, JSON.stringify(views));
    } catch {
      // Remembered for this visit only.
    }
  };
  const investing = report?.id === "investments";
  const forecasting = view?.id === "forecast";
  // The overview's first cards are of spending by category too, over its own six months.
  const byCategory = !view || !!view.categories;
  const own = !!view?.own;
  const [result, setResult] = useState<{ budget: Budget; key: string; attempt: number; report?: ReportData; error?: string } | null>(null);
  const [attempt, setAttempt] = useState(0);

  // Groups that can hold spending, in budget order; colours follow the group, not its rank.
  // Spending with no category reads as its own group, in the language in use.
  const needsCategory = t(NEEDS_CATEGORY);
  const reportCategories = useMemo<Budget["categories"]>(() => [...budget.categories, {
    id: UNCATEGORIZED_CATEGORY, name: needsCategory, group: needsCategory, target: 0,
  }], [budget.categories, needsCategory]);
  const groupOf = useMemo(() => new Map(reportCategories.map((c) => [c.id, c.group])), [reportCategories]);
  const nameOf = useMemo(() => new Map(reportCategories.map((c) => [c.id, c.name])), [reportCategories]);
  const spendingGroups = useMemo(
    () => [...new Set([...groupNames(budget).filter((g) => budget.categories.some((c) => c.group === g && !c.paymentAccountId)), needsCategory])],
    [budget.categories, budget.groupOrder, needsCategory, language],
  );
  const accountGroups: FilterGroup[] = [
    { name: msg("Budget"), items: budget.accounts.filter((a) => !a.archived && a.kind !== "tracking") },
    { name: msg("Tracking"), items: budget.accounts.filter((a) => !a.archived && a.kind === "tracking") },
    { name: msg("Closed"), items: budget.accounts.filter((a) => a.archived) },
  ]
    // The forecast is of budget accounts, and the investment views of accounts with holdings.
    .map((g) => ({ ...g, items: g.items.filter((a) => !investing || held.has(a.id)) }))
    .filter((g) => g.items.length && (!forecasting || g.name === "Budget"))
    .map((g) => ({ name: t(g.name), items: g.items.map((a) => ({ id: a.id, label: a.name })) }));
  const categoryGroups: FilterGroup[] = spendingGroups.map((g) => ({
    name: g,
    items: reportCategories
      .filter((c) => c.group === g && !c.paymentAccountId)
      .map((c) => ({ id: c.id, label: c.name })),
  }));
  // What reports leave out is remembered; the filter sends what they show. Ids of deleted
  // accounts and categories are dropped when the choice next changes.
  const accounts = chosen(hidden.accounts, accountGroups.flatMap((g) => g.items.map((i) => i.id)));
  const categoryIds = categoryGroups.flatMap((g) => g.items.map((i) => i.id));
  const categories = byCategory ? chosen(hidden.categories, categoryIds) : new Set<string>();
  const hideAccounts = (next: Set<string>) =>
    setHidden({ accounts: new Set([...next].filter((id) => budget.accounts.some((a) => a.id === id))) });
  const hideCategories = (next: Set<string>) => setHidden({ categories: new Set([...next].filter((id) => categoryIds.includes(id))) });

  // The overview covers the last six months; a view, the months chosen. Views that don't split
  // by category ignore the Categories filter.
  const thisMonth = today().slice(0, 7);
  const filter: ReportFilter = {
    from: view ? range.from || earliest : shiftMonth(thisMonth, -5),
    to: view ? range.to || thisMonth : thisMonth,
    accountIds: [...accounts],
    categoryIds: byCategory ? [...categories] : [],
  };
  const key = JSON.stringify(filter);
  const matches = result?.budget === budget && result.key === key && result.attempt === attempt;
  const data = matches ? result.report ?? null : null;
  const error = matches ? result.error : undefined;
  const loading = !data && !error;
  useEffect(() => {
    if (own) return;
    let cancelled = false;
    const controller = new AbortController();
    // A report belongs to its exact snapshot and filters; new labels must never describe old figures.
    read<ReportData>(budget, "calculate_reports", { filter: JSON.parse(key) }, controller.signal)
      .then((r) => {
        if (cancelled) return;
        setResult({ budget, key, attempt, report: r });
      })
      .catch((e) => !cancelled && setResult({ budget, key, attempt, error: errorText(e) }));
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [budget, key, own, attempt]);

  const colorOf = (group: string) => {
    const i = spendingGroups.indexOf(group);
    return i >= 0 && i < 7 ? SERIES(i) : OTHER;
  };
  const inspect = (f: RegisterFilter) =>
    onInspect({ ...f, from: f.from ?? firstDay(filter.from), to: f.to ?? lastDay(filter.to) });
  // The figure whose transactions are listed in a dialog, and the query that finds them.
  const [selection, setSelection] = useState<{ budget: Budget; key: string; figure: Figure } | null>(null);
  const figure = data && view && !own && selection?.budget === budget && selection.key === key ? selection.figure : null;
  const setFigure = (figure: Figure | null) => setSelection(figure ? { budget, key, figure } : null);
  const chosenCategories = byCategory && categories.size ? new Set([...categories].filter(Boolean)) : undefined;
  const lineQuery: LineQuery | null = figure && {
    from: figure.from ?? firstDay(filter.from),
    to: figure.to ?? lastDay(filter.to),
    payee: figure.payee,
    income: figure.income,
    accountIds: accounts,
    categoryIds: figure.categoryId ? new Set([figure.categoryId]) : figure.income ? undefined : chosenCategories,
  };

  const exportCsv = async () => {
    const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
    try {
      const path = await (investing ? api.exportInvestments : api.exportReports)(budget, filter);
      if (path) toast({ message: t("Report exported."), action: { label: t("Show in folder"), onClick: () => void api.reveal(path).catch(fail) } });
    } catch (e) {
      fail(e);
    }
  };

  const scope: Scope | null = data && {
    data,
    budget,
    from: filter.from,
    to: filter.to,
    accounts,
    someCategories: byCategory ? categories.size > 0 : false,
    groupOf,
    nameOf,
    spendingGroups,
    colorOf,
    inspect,
    show: setFigure,
  };
  const body = (s: Scope) => {
    switch (view?.id) {
      case "categories":
        return <ByCategory scope={s} />;
      case "payees":
        return <ByPayee scope={s} />;
      case "trends":
        return <OverTime scope={s} />;
      case "calendar":
        return <SpendingCalendar scope={s} />;
      case "incomeExpense":
        return <IncomeExpense scope={s} />;
      case "sources":
        return <IncomeBreakdown scope={s} />;
      case "cashflow":
        return <CashFlow scope={s} />;
      case "networth":
        return <NetWorth scope={s} />;
      case "balance":
        return <BalanceSheet scope={s} />;
      case "balances":
        return <Balances scope={s} />;
      case "age":
        return <AgeOfMoney scope={s} />;
      default:
        return null;
    }
  };
  // What is in force, for the filter's label: the months where the view has any, the accounts,
  // and the categories once some are left out.
  const narrowedAccounts = accountGroups.some((g) => g.items.some((i) => hidden.accounts.has(i.id)));
  const narrowedCategories = byCategory && categoryGroups.some((g) => g.items.some((i) => hidden.categories.has(i.id)));
  const inForce = [
    view && !view.dateless ? describeRange(range, "month") : "",
    shown(accountGroups, hidden.accounts, forecasting ? t("All budget accounts") : t("All accounts"), (n) => tn(n, "{count} accounts", "{count} accounts")),
    narrowedCategories ? shown(categoryGroups, hidden.categories, "", (n) => tn(n, "{count} categories", "{count} categories")) : "",
  ]
    .filter(Boolean)
    .join(" · ");
  // Money moved into or out of investment accounts is neither spending nor income: the reports
  // on those say how much of it they leave out.
  const invested = data && (report?.id === "spending" || report?.id === "income") ? (data.invested ?? 0) : 0;
  const investedNote =
    invested > 0
      ? t("{amount} moved into investments isn't counted here.", { amount: fmt(invested) })
      : invested < 0
        ? t("{amount} taken out of investments isn't counted here.", { amount: fmt(-invested) })
        : undefined;
  const leftOut = (
    <>
      <LeftOut kind="accounts" groups={accountGroups} hidden={hidden.accounts} onChange={hideAccounts} note={investedNote} />
      {byCategory && <LeftOut kind="categories" groups={categoryGroups} hidden={hidden.categories} onChange={hideCategories} />}
      {error && !own && <p className="form-error" role="alert">{error} <button className="link-button" onClick={() => setAttempt((n) => n + 1)}>{t("Retry report")}</button></p>}
    </>
  );

  return (
    <div className="page reports-page">
      <div className="reports-layout">
        {/* The overview and the reports are one row of tabs in the page header. */}
        <HeaderSlot>
          <Tabs<Place>
            label={t("Reports")}
            tabs={[{ id: "overview", label: t("Overview") }, ...reports.map((r) => ({ id: r.id, label: t(r.label) }))]}
            value={report?.id ?? "overview"}
            onChange={(place) => go(place)}
          />
        </HeaderSlot>
        {!report || !view ? (
          <div className="report-main">
            {leftOut}
            <Overview
              cards={reports}
              budget={budget}
              data={data}
              from={filter.from}
              to={filter.to}
              accounts={accounts}
              hidden={hidden.accounts}
              open={(id) => go(id)}
            />
          </div>
        ) : (
        <div className="report-main">
          <header className="report-head">
            <h1>{t(report.label)}</h1>
            <p>{t(report.question)}</p>
          </header>
          {/* The report's views, and what the one chosen shows. */}
          <div className="report-view-row">
            {report.views.length > 1 && (
              <Tabs<ViewId>
                key={report.id}
                label={t("View")}
                tabs={report.views.map((v) => ({ id: v.id, label: t(v.label) }))}
                value={view.id}
                onChange={(id) => go(report.id, id)}
              />
            )}
            <p className="report-blurb">{t(view.blurb)}</p>
          </div>
          <div className="report-filter-row">
            <Filters label={inForce} narrowed={narrowedAccounts || narrowedCategories}>
              {!view.dateless && (
                <DateRangePicker
                  value={range}
                  onChange={setRange}
                  granularity="month"
                  earliest={earliest}
                  label={t("Months")}
                />
              )}
              <MultiFilter
                label={t("Accounts")}
                allLabel={forecasting ? t("All budget accounts") : t("All accounts")}
                searchLabel={t("Search accounts")}
                countLabel={(n) => tn(n, "{count} accounts", "{count} accounts")}
                hiddenLabel={(n) => tnc("accounts", n, "{count} hidden", "{count} hidden")}
                groups={accountGroups}
                hidden={hidden.accounts}
                onChange={hideAccounts}
              />
              {view.categories && (
                <MultiFilter
                  label={t("Categories")}
                  allLabel={t("All categories")}
                  searchLabel={t("Search categories")}
                  countLabel={(n) => tn(n, "{count} categories", "{count} categories")}
                  hiddenLabel={(n) => tnc("categories", n, "{count} hidden", "{count} hidden")}
                  groups={categoryGroups}
                  hidden={hidden.categories}
                  onChange={hideCategories}
                />
              )}
            </Filters>
            {loading && !own && <LoaderCircle size={16} className="spin muted" aria-label={t("Updating")} />}
            <span className="spacer" />
            {!forecasting && (
              <button className="secondary export-button" onClick={() => void exportCsv()}>
                <Download size={15} /> {t("Export CSV")}
              </button>
            )}
          </div>
          {leftOut}
          {figure && lineQuery && (
          <TransactionsDialog
            budget={budget}
            title={figure.title}
            query={lineQuery}
            onClose={() => setFigure(null)}
            onOpenRegister={() => {
              setFigure(null);
              inspect({ reportQuery: lineQuery, from: lineQuery.from, to: lineQuery.to });
            }}
          />
        )}
        {forecasting ? (
            <Forecast budget={budget} accounts={accounts} inspect={inspect} />
          ) : investing ? (
            <Investing
              report={view.id as InvestmentsReport}
              budget={budget}
              filter={{ from: filter.from, to: filter.to, accountIds: filter.accountIds }}
              onOpenAccount={onOpenAccount}
            />
          ) : !scope ? (error ? null : (
            <div className="empty-state compact">
              <LoaderCircle className="spin" />
            </div>
          )) : (
            <div className={`report-body${loading ? " loading" : ""}`} key={view.id}>
              {!!scope.data.categories[UNCATEGORIZED_CATEGORY] && view.categories && (
                <p className="notice">
                  {t("{amount} of spending still needs a category. It is included in these totals.", {
                    amount: money(scope.data.categories[UNCATEGORIZED_CATEGORY], { symbol: true }),
                  })}{" "}
                  <button className="link-button" onClick={() => setFigure({ title: needsCategory, categoryId: UNCATEGORIZED_CATEGORY })}>{t("Review transactions")}</button>
                </p>
              )}
              {body(scope)}
            </div>
          )}
        </div>
        )}
      </div>
    </div>
  );
}
