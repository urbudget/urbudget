import type { ReactNode } from "react";
import { LoaderCircle, type LucideIcon } from "lucide-react";
import { Spark } from "../charts/Charts";
import { formatDate, shiftDate } from "../format";
import { t } from "../i18n";
import { today, type Budget } from "../types";
import { balanceSheet } from "./balanceSheet";
import { chosen } from "./filters";
import { forecastLine, projection, useForecast } from "./Forecast";
import { holdingAccounts, usePortfolio } from "./portfolio";
import { SERIES, fmt, signed, sum, type ReportData } from "./shared";
import { openingNetWorth } from "./Wealth";

/** A report as its card names it. */
export type Card<K extends string> = { id: K; label: string; question: string; icon: LucideIcon };
/** What a card shows of its report: one figure, a few words about it, and a small picture. */
type Headline = { figure: string; tone?: "negative" | "positive"; note: string; chart: ReactNode };
/** How far ahead the forecast's card looks, as the report does when it opens. */
const AHEAD = 6;

/**
 * Where Reports starts: a card for each report with its question, its headline figure and a
 * small chart, over the six months `data` covers. A card opens its report. The first four read
 * `data`; investments and the forecast read their own figures, as their reports do.
 */
export function Overview<K extends string>({
  cards,
  budget,
  data,
  from,
  to,
  accounts,
  hidden,
  open,
}: {
  cards: Card<K>[];
  budget: Budget;
  /** The shared report for the six months, or null while it is read. */
  data: ReportData | null;
  from: string;
  to: string;
  /** The accounts `data` is of; empty means all. */
  accounts: Set<string>;
  /** The accounts reports leave out, for the cards that read their own figures. */
  hidden: Set<string>;
  open: (id: K) => void;
}) {
  const invested = holdingAccounts(budget).map((a) => a.id);
  const portfolio = usePortfolio(budget, { from, to, accountIds: [...chosen(hidden, invested)] }, invested.length > 0);
  // The forecast is of budget accounts, from today.
  const spendable = chosen(hidden, budget.accounts.filter((a) => !a.archived && a.kind !== "tracking").map((a) => a.id));
  const start = today();
  const through = shiftDate(start, 0, AHEAD);
  const forecast = useForecast(budget, spendable, start, through);
  const months = data?.months ?? [];
  const last = months[months.length - 1];
  const lines = (values: number[]) => <Spark kind="lines" series={[{ color: SERIES(0), values }]} />;

  const headline = (id: string): Headline | null => {
    switch (id) {
      case "spending":
        return last
          ? {
              figure: fmt(last.spending),
              note: `${t("This month")} · ${t("Monthly average")} ${fmt(Math.round(sum(months.map((m) => m.spending)) / months.length))}`,
              chart: <Spark kind="columns" series={[{ color: SERIES(0), values: months.map((m) => m.spending) }]} />,
            }
          : null;
      case "income": {
        if (!data || !last) return null;
        const left = data.income - data.spending;
        return {
          figure: signed(left),
          tone: left < 0 ? "negative" : left > 0 ? "positive" : undefined,
          note: t("Last 6 months"),
          chart: (
            <Spark
              kind="columns"
              series={[
                { color: SERIES(0), values: months.map((m) => m.income) },
                { color: SERIES(1), values: months.map((m) => m.spending) },
              ]}
            />
          ),
        };
      }
      case "networth": {
        if (!data || !last) return null;
        const before = openingNetWorth(data, budget.accounts.filter((a) => !accounts.size || accounts.has(a.id)));
        return {
          figure: fmt(last.netWorth),
          tone: last.netWorth < 0 ? "negative" : undefined,
          note: `${signed(last.netWorth - before)} · ${t("Last 6 months")}`,
          chart: lines(months.map((m) => m.netWorth)),
        };
      }
      case "balance": {
        if (!data || !last) return null;
        const { assets, debts } = balanceSheet(data, budget.accounts, accounts, null).totals;
        return {
          figure: fmt(assets.end),
          note: `${t("Total liabilities")} ${fmt(debts.end)}`,
          // One bar, split into what is owned and what is owed.
          chart: (
            <div className="split-bar" aria-hidden>
              {assets.end > 0 && <i style={{ flexGrow: assets.end, background: SERIES(0) }} />}
              {debts.end > 0 && <i style={{ flexGrow: debts.end, background: SERIES(1) }} />}
            </div>
          ),
        };
      }
      case "investments": {
        const p = portfolio.data;
        return p
          ? { figure: fmt(p.total.end), note: `${t("Gain")} ${signed(p.total.gain)}`, chart: lines(p.months.map((m) => m.value)) }
          : null;
      }
      case "forecast": {
        if (!forecast.data) return null;
        const ahead = projection(forecast.data, spendable, start, through, AHEAD);
        const line = forecastLine(ahead, ahead.suggested);
        const low = line.reduce((m, v, i) => (v < line[m] ? i : m), 0);
        return {
          figure: fmt(line[low]),
          tone: line[low] < 0 ? "negative" : undefined,
          note: `${t("Lowest balance ahead")} · ${low === 0 ? t("today") : formatDate(ahead.days[low])}`,
          chart: lines(line),
        };
      }
      default:
        return null;
    }
  };
  // A card whose figures couldn't be read says so with a dash; the others are on their way.
  const failed = (id: string) => (id === "investments" ? !!portfolio.error : id === "forecast" ? !!forecast.error : false);
  return (
    <div className="report-cards">
      {cards.map((card) => {
        const shown = headline(card.id);
        return (
          <article key={card.id} className="report-card">
            <h2>
              <button onClick={() => open(card.id)}>
                <card.icon size={15} aria-hidden />
                {t(card.label)}
              </button>
            </h2>
            <p className="report-card-question">{t(card.question)}</p>
            {shown ? (
              <>
                <p className={`report-card-figure ${shown.tone ?? ""}`}>{shown.figure}</p>
                <p className="report-card-note">{shown.note}</p>
                {shown.chart}
              </>
            ) : (
              <p className="report-card-figure muted">{failed(card.id) || data?.months.length === 0 ? "—" : <LoaderCircle size={18} className="spin" />}</p>
            )}
          </article>
        );
      })}
    </div>
  );
}
