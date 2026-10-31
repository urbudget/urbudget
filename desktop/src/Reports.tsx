import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeftRight,
  CalendarDays,
  ChartColumnStacked,
  ChartLine,
  ChartPie,
  ChevronDown,
  Download,
  EyeOff,
  HandCoins,
  Hourglass,
  Landmark,
  LoaderCircle,
  Scale,
  Store,
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
  rangeFor,
  useToast,
  type DateRange,
} from "./ui";
import { today, type Budget } from "./types";
import type { RegisterFilter } from "./Register";
import { OTHER, SERIES, firstDay, lastDay, type Figure, type ReportData, type Scope } from "./reports/shared";
import { TransactionsDialog, type LineQuery } from "./reports/TransactionsDialog";
import { ByCategory, ByPayee, OverTime, SpendingCalendar } from "./reports/Spending";
import { CashFlow, IncomeBreakdown, IncomeExpense } from "./reports/Income";
import { AgeOfMoney, BalanceSheet, Balances, NetWorth } from "./reports/Wealth";
import { Forecast } from "./reports/Forecast";
import { chosen, useHidden } from "./reports/filters";
import { msg, t, tn, tnc, useLanguage } from "./i18n";
import "./reports.css";

type ReportId =
  | "categories"
  | "payees"
  | "trends"
  | "calendar"
  | "incomeExpense"
  | "income"
  | "cashflow"
  | "networth"
  | "balance"
  | "balances"
  | "forecast"
  | "age";
type ReportInfo = {
  id: ReportId;
  /** The report's full name, its page heading. */
  label: string;
  /** Its tab's name, where the group (Spending) already says the rest; else the label. */
  short?: string;
  icon: LucideIcon;
  blurb: string;
  /** The Categories filter applies. */
  categories?: boolean;
};
// Group names are also keys (remembered views); names, tabs and blurbs are translated when shown.
const REPORTS: { group: string; items: ReportInfo[] }[] = [
  {
    group: msg("Spending"),
    items: [
      {
        id: "categories",
        label: msg("Spending by category"),
        short: msg("By category"),
        icon: ChartPie,
        blurb: msg("Where your money went, by group and category, and how last month compares."),
        categories: true,
      },
      {
        id: "payees",
        label: msg("Spending by payee"),
        short: msg("By payee"),
        icon: Store,
        blurb: msg("Who you pay most, and how often."),
        categories: true,
      },
      {
        id: "trends",
        label: msg("Spending over time"),
        short: msg("Over time"),
        icon: ChartColumnStacked,
        blurb: msg("Spending month by month, by group or in total."),
        categories: true,
      },
      {
        id: "calendar",
        label: msg("Spending calendar"),
        short: msg("Calendar"),
        icon: CalendarDays,
        blurb: msg("Spending day by day: busy days, quiet days and weekdays."),
        categories: true,
      },
    ],
  },
  {
    group: msg("Income"),
    items: [
      {
        id: "incomeExpense",
        label: msg("Income vs. expense"),
        icon: Scale,
        blurb: msg("Income, spending and the surplus left in your budget."),
        categories: true,
      },
      {
        id: "income",
        label: msg("Income breakdown"),
        icon: HandCoins,
        blurb: msg("Where your income comes from, and how steady it is."),
      },
      {
        id: "cashflow",
        label: msg("Money in and out"),
        icon: ArrowLeftRight,
        blurb: msg("Account movements across your budget boundary, including credit purchases."),
        categories: true,
      },
    ],
  },
  {
    group: msg("Wealth"),
    items: [
      {
        id: "networth",
        label: msg("Net worth"),
        icon: TrendingUp,
        blurb: msg("What you own minus what you owe, month by month."),
      },
      {
        id: "balance",
        label: msg("Balance sheet"),
        icon: Landmark,
        blurb: msg("Assets and liabilities by type, with the ratios that matter."),
      },
      {
        id: "balances",
        label: msg("Balance over time"),
        icon: ChartLine,
        blurb: msg("Each account's balance at the end of every month."),
      },
      {
        id: "forecast",
        label: msg("Forecast"),
        icon: Telescope,
        blurb: msg("Where your cash and card balances are heading, from what's scheduled."),
      },
      {
        id: "age",
        label: msg("Cash age"),
        icon: Hourglass,
        blurb: msg("How long money waits in your accounts before you spend it."),
      },
    ],
  },
];
const ALL = REPORTS.flatMap((g) => g.items);
const groupOfReport = (id: ReportId) => REPORTS.find((g) => g.items.some((r) => r.id === id))!;
const VIEW_KEY = "lb:reports:view";
/** The last report opened in each group, so going back to a group reopens it. */
const GROUP_VIEWS_KEY = "lb:reports:groupViews";

type FilterGroup = { name: string; items: { id: string; label: string }[] };

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
}: {
  kind: "accounts" | "categories";
  groups: FilterGroup[];
  hidden: Set<string>;
  onChange: (hidden: Set<string>) => void;
}) {
  const all = groups.flatMap((g) => g.items);
  const off = all.filter((i) => hidden.has(i.id));
  if (!off.length) return null;
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
    </div>
  );
}

export function Reports({
  budget,
  budgetId,
  month,
  onInspect,
}: {
  budget: Budget;
  /** Keys what the reports leave out, which each budget remembers. */
  budgetId: string;
  month: string;
  /** Opens the register on these transactions. */
  onInspect: (filter: RegisterFilter) => void;
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
  const [view, setView] = useState<ReportId>(() => {
    try {
      const saved = localStorage.getItem(VIEW_KEY);
      if (ALL.some((r) => r.id === saved)) return saved as ReportId;
    } catch {
      // No storage: start on the first report.
    }
    return "categories";
  });
  const [groupViews, setGroupViews] = useState<Record<string, string>>(() => {
    try {
      return { ...JSON.parse(localStorage.getItem(GROUP_VIEWS_KEY) ?? "{}"), [groupOfReport(view).group]: view };
    } catch {
      return { [groupOfReport(view).group]: view };
    }
  });
  const choose = (id: ReportId) => {
    const views = { ...groupViews, [groupOfReport(id).group]: id };
    setView(id);
    setGroupViews(views);
    try {
      localStorage.setItem(VIEW_KEY, id);
      localStorage.setItem(GROUP_VIEWS_KEY, JSON.stringify(views));
    } catch {
      // Remembered for this visit only.
    }
  };
  const group = groupOfReport(view);
  const chooseGroup = (name: string) => {
    const items = REPORTS.find((g) => g.group === name)!.items;
    choose(items.find((r) => r.id === groupViews[name])?.id ?? items[0].id);
  };
  const info = ALL.find((r) => r.id === view)!;
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
    .filter((g) => g.items.length && (view !== "forecast" || g.name === "Budget"))
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
  const categories = info.categories ? chosen(hidden.categories, categoryIds) : new Set<string>();
  const hideAccounts = (next: Set<string>) =>
    setHidden({ accounts: new Set([...next].filter((id) => budget.accounts.some((a) => a.id === id))) });
  const hideCategories = (next: Set<string>) => setHidden({ categories: new Set([...next].filter((id) => categoryIds.includes(id))) });

  // Reports that don't split by category ignore the Categories filter.
  const filter: ReportFilter = {
    from: range.from || earliest,
    to: range.to || today().slice(0, 7),
    accountIds: [...accounts],
    categoryIds: info.categories ? [...categories] : [],
  };
  const key = JSON.stringify(filter);
  const matches = result?.budget === budget && result.key === key && result.attempt === attempt;
  const report = matches ? result.report ?? null : null;
  const error = matches ? result.error : undefined;
  const loading = !report && !error;
  useEffect(() => {
    if (view === "forecast") return;
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
  }, [budget, key, view === "forecast", attempt]);

  const colorOf = (group: string) => {
    const i = spendingGroups.indexOf(group);
    return i >= 0 && i < 7 ? SERIES(i) : OTHER;
  };
  const inspect = (f: RegisterFilter) =>
    onInspect({ ...f, from: f.from ?? firstDay(filter.from), to: f.to ?? lastDay(filter.to) });
  // The figure whose transactions are listed in a dialog, and the query that finds them.
  const [selection, setSelection] = useState<{ budget: Budget; key: string; figure: Figure } | null>(null);
  const figure = report && view !== "forecast" && selection?.budget === budget && selection.key === key ? selection.figure : null;
  const setFigure = (figure: Figure | null) => setSelection(figure ? { budget, key, figure } : null);
  const chosenCategories = info.categories && categories.size ? new Set([...categories].filter(Boolean)) : undefined;
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
      const path = await api.exportReports(budget, filter);
      if (path) toast({ message: t("Report exported."), action: { label: t("Show in folder"), onClick: () => void api.reveal(path).catch(fail) } });
    } catch (e) {
      fail(e);
    }
  };

  const scope: Scope | null = report && {
    data: report,
    budget,
    from: filter.from,
    to: filter.to,
    accounts,
    someCategories: info.categories ? categories.size > 0 : false,
    groupOf,
    nameOf,
    spendingGroups,
    colorOf,
    inspect,
    show: setFigure,
  };
  const body = (s: Scope) => {
    switch (view) {
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
      case "income":
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

  return (
    <div className="page reports-page">
      <div className="reports-layout">
        {/* The groups switch in the page header, and the group's reports are tabs over the report. */}
        <HeaderSlot>
          <Tabs
            label={t("Report group")}
            tabs={REPORTS.map((g) => ({ id: g.group, label: t(g.group) }))}
            value={group.group}
            onChange={chooseGroup}
          />
        </HeaderSlot>
        <div className="report-nav">
          <Tabs
            key={group.group}
            className="report-tabs"
            label={t("{group} reports", { group: t(group.group) })}
            tabs={group.items.map((r) => ({
              id: r.id,
              label: (
                <>
                  <r.icon size={15} aria-hidden />
                  {t(r.short ?? r.label)}
                </>
              ),
            }))}
            value={view}
            onChange={choose}
          />
        </div>
        <div className="report-main">
          <header className="report-head">
            <h1>{t(info.label)}</h1>
            <p>{t(info.blurb)}</p>
          </header>
          <div className="report-filter-row">
            {view !== "forecast" && (
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
              allLabel={view === "forecast" ? t("All budget accounts") : t("All accounts")}
              searchLabel={t("Search accounts")}
              countLabel={(n) => tn(n, "{count} accounts", "{count} accounts")}
              hiddenLabel={(n) => tnc("accounts", n, "{count} hidden", "{count} hidden")}
              groups={accountGroups}
              hidden={hidden.accounts}
              onChange={hideAccounts}
            />
            {info.categories && (
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
            {loading && view !== "forecast" && <LoaderCircle size={16} className="spin muted" aria-label={t("Updating")} />}
            <span className="spacer" />
            {view !== "forecast" && (
              <button className="secondary export-button" onClick={() => void exportCsv()}>
                <Download size={15} /> {t("Export CSV")}
              </button>
            )}
          </div>
          <LeftOut kind="accounts" groups={accountGroups} hidden={hidden.accounts} onChange={hideAccounts} />
          {info.categories && <LeftOut kind="categories" groups={categoryGroups} hidden={hidden.categories} onChange={hideCategories} />}
          {error && view !== "forecast" && <p className="form-error" role="alert">{error} <button className="link-button" onClick={() => setAttempt((n) => n + 1)}>{t("Retry report")}</button></p>}
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
        {view === "forecast" ? (
            <Forecast budget={budget} accounts={accounts} inspect={inspect} />
          ) : !scope ? (error ? null : (
            <div className="empty-state compact">
              <LoaderCircle className="spin" />
            </div>
          )) : (
            <div className={`report-body${loading ? " loading" : ""}`} key={view}>
              {!!scope.data.categories[UNCATEGORIZED_CATEGORY] && info.categories && (
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
      </div>
    </div>
  );
}
