import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeftRight,
  CalendarDays,
  ChartColumnStacked,
  ChartLine,
  ChartPie,
  ChevronDown,
  Download,
  HandCoins,
  Hourglass,
  Landmark,
  LoaderCircle,
  Scale,
  Store,
  Telescope,
  TrendingUp,
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
import { msg, t, tn, useLanguage } from "./i18n";
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

/** A filter button with a searchable, grouped checklist. An empty selection means "all". */
function MultiFilter({
  label,
  allLabel,
  searchLabel,
  countLabel,
  groups,
  selected,
  onChange,
}: {
  label: string;
  allLabel: string;
  /** The search field's placeholder, such as "Search accounts". */
  searchLabel: string;
  /** How several chosen items read, such as "3 accounts". */
  countLabel: (count: number) => string;
  groups: { name: string; items: { id: string; label: string }[] }[];
  selected: Set<string>;
  onChange: (ids: Set<string>) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const all = groups.flatMap((g) => g.items);
  // An empty set means all; a set with only unknown ids (e.g. "") means none.
  const chosen = all.filter((i) => selected.has(i.id));
  const shown = groups
    .map((g) => ({ ...g, items: g.items.filter((i) => i.label.toLowerCase().includes(query.toLowerCase())) }))
    .filter((g) => g.items.length);
  const summary = !selected.size
    ? allLabel
    : !chosen.length
      ? t("None")
      : chosen.length === 1
        ? chosen[0].label
        : countLabel(chosen.length);
  const toggle = (ids: string[], on: boolean) => {
    const next = new Set(selected.size ? chosen.map((i) => i.id) : all.map((i) => i.id));
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    onChange(next.size === all.length ? new Set() : next.size ? next : new Set([""]));
  };
  const isOn = (id: string) => !selected.size || selected.has(id);
  return (
    <>
      <button ref={button} className="secondary filter-button" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="muted">{label}:</span> {summary} <ChevronDown size={14} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={label} className="filter-popover">
        <div className="filter-popover-head">
          <input
            placeholder={searchLabel}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button className="link-button" onClick={() => onChange(new Set())}>
            {t("All")}
          </button>
          <button className="link-button" onClick={() => onChange(new Set([""]))}>
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
                  <input type="checkbox" checked={every} onChange={() => toggle(ids, !every)} />
                  {g.name}
                </label>
                {g.items.map((i) => (
                  <label key={i.id} className="checkbox-label">
                    <input type="checkbox" checked={isOn(i.id)} onChange={(e) => toggle([i.id], e.target.checked)} />
                    {i.label}
                  </label>
                ))}
              </fieldset>
            );
          })}
        </div>
      </Popover>
    </>
  );
}

export function Reports({
  budget,
  month,
  onInspect,
}: {
  budget: Budget;
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
  const [accounts, setAccounts] = useState<Set<string>>(new Set());
  const [categories, setCategories] = useState<Set<string>>(new Set());
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

  const accountGroups = [
    { name: msg("Budget"), items: budget.accounts.filter((a) => !a.archived && a.kind !== "tracking") },
    { name: msg("Tracking"), items: budget.accounts.filter((a) => !a.archived && a.kind === "tracking") },
    { name: msg("Closed"), items: budget.accounts.filter((a) => a.archived) },
  ]
    .filter((g) => g.items.length && (view !== "forecast" || g.name === "Budget"))
    .map((g) => ({ name: t(g.name), items: g.items.map((a) => ({ id: a.id, label: a.name })) }));
  const categoryGroups = spendingGroups.map((g) => ({
    name: g,
    items: reportCategories
      .filter((c) => c.group === g && !c.paymentAccountId)
      .map((c) => ({ id: c.id, label: c.name })),
  }));

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
              groups={accountGroups}
              selected={accounts}
              onChange={setAccounts}
            />
            {info.categories && (
              <MultiFilter
                label={t("Categories")}
                allLabel={t("All categories")}
                searchLabel={t("Search categories")}
                countLabel={(n) => tn(n, "{count} categories", "{count} categories")}
                groups={categoryGroups}
                selected={categories}
                onChange={setCategories}
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
