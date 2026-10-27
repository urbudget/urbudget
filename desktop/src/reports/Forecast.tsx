import { useEffect, useMemo, useRef, useState } from "react";
import { LoaderCircle } from "lucide-react";
import { read } from "../bridge";
import { Legend, LinesChart } from "../charts/Charts";
import { editableAmount, errorText, evaluateAmount, formatDate, money, shiftDate } from "../format";
import { Tabs, useVirtualRows } from "../ui";
import { today, type Budget } from "../types";
import type { RegisterFilter } from "../Register";
import { Panel, SERIES, Tiles, fmt, signed, sum, useReportColumns } from "./shared";
import { monthNames, t, tn } from "../i18n";

type Event = {
  date: string;
  amount: number;
  payee: string;
  accountId: string;
  peerId: string | null;
  categoryId: string | null;
  scheduleId: string | null;
};
type ForecastData = { balances: Record<string, number>; events: Event[]; recentSpending: number };
const ROW = 37;

/**
 * Where the money in budget accounts is heading: today's balance, every transaction already
 * entered for a later date and every scheduled one, plus an even amount of everyday spending.
 */
export function Forecast({
  budget,
  accounts,
  inspect,
}: {
  budget: Budget;
  accounts: Set<string>;
  inspect: (filter: RegisterFilter) => void;
}) {
  const [ahead, setAhead] = useState<"3" | "6" | "12">("6");
  const [everyday, setEveryday] = useState<number | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [focused, setFocused] = useState(false);
  const [result, setResult] = useState<{ budget: Budget; key: string; attempt: number; data?: ForecastData; error?: string } | null>(null);
  const [attempt, setAttempt] = useState(0);
  const start = today();
  const through = shiftDate(start, 0, Number(ahead));
  const accountKey = JSON.stringify([...accounts].sort());
  const key = JSON.stringify([start, through, accountKey]);
  const matches = result?.budget === budget && result.key === key && result.attempt === attempt;
  const data = matches ? result.data : undefined;
  const error = matches ? result.error : undefined;
  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    read<ForecastData>(budget, "forecast", { today: start, through, accountIds: JSON.parse(accountKey) }, controller.signal)
      .then((d) => {
        if (cancelled) return;
        setResult({ budget, key, attempt, data: d });
      })
      .catch((e) => !cancelled && setResult({ budget, key, attempt, error: errorText(e) }));
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [budget, start, through, accountKey, key, attempt]);

  const view = useMemo(() => {
    if (!data) return null;
    const ids = new Set(Object.keys(data.balances).filter((id) => !accounts.size || accounts.has(id)));
    // Moves between two of the chosen accounts don't change their total.
    const events = data.events.filter((e) => ids.has(e.accountId) && !(e.peerId && ids.has(e.peerId)));
    const opening = sum([...ids].map((id) => data.balances[id] ?? 0));
    const months = Number(ahead);
    const scheduledSpending =
      sum(events.filter((e) => e.scheduleId && e.categoryId && e.amount < 0).map((e) => -e.amount)) / months;
    const suggested = Math.max(0, Math.round(data.recentSpending - scheduledSpending));
    const days: string[] = [];
    for (let d = start; d <= through; d = shiftDate(d, 1)) days.push(d);
    const byDay = new Map<string, number>();
    for (const e of events) byDay.set(e.date, (byDay.get(e.date) ?? 0) + e.amount);
    let known = opening;
    const knownLine = days.map((d) => (known += byDay.get(d) ?? 0));
    let running = opening;
    const balances = events.map((e) => (running += e.amount));
    return { ids, events, opening, suggested, scheduledSpending, days, knownLine, balances };
  }, [data, accounts, ahead, start, through]);

  const scroller = useRef<HTMLDivElement>(null);
  const rows = useVirtualRows(scroller, view?.events.length ?? 0, ROW);
  const table = useReportColumns("forecast", 5, 130);
  const rangeControl = <Tabs<"3" | "6" | "12">
    label={t("How far ahead")} value={ahead} onChange={setAhead}
    tabs={[
      { id: "3", label: tn(3, "{count} month", "{count} months") },
      { id: "6", label: tn(6, "{count} month", "{count} months") },
      { id: "12", label: tn(12, "{count} month", "{count} months") },
    ]}
  />;
  if (!view || !data)
    return (
      <div className="report-body">
        <Panel title={t("Balance ahead")} aside={rangeControl}>
          {error ? <p className="form-error" role="alert">{error} <button className="link-button" onClick={() => setAttempt((n) => n + 1)}>{t("Retry forecast")}</button></p>
            : <div className="empty-state compact"><LoaderCircle className="spin" /></div>}
        </Panel>
      </div>
    );
  const monthly = everyday ?? view.suggested;
  const perDay = (monthly * 12) / 365;
  const forecast = view.knownLine.map((v, i) => Math.round(v - perDay * i));
  const low = forecast.reduce((m, v, i) => (v < forecast[m] ? i : m), 0);
  const end = forecast[forecast.length - 1];
  const name = (id: string) => budget.accounts.find((a) => a.id === id)?.name ?? "";
  const category = (id: string | null) => (id ? (budget.categories.find((c) => c.id === id)?.name ?? "") : "");
  const knownIn = sum(view.events.filter((e) => e.amount > 0).map((e) => e.amount));
  const knownOut = sum(view.events.filter((e) => e.amount < 0).map((e) => -e.amount));
  const commit = () => {
    if (draft === null) return;
    try {
      const v = evaluateAmount(draft);
      setEveryday(Math.max(0, Math.abs(v)));
    } catch {
      // Keep the last good amount.
    }
    setDraft(null);
  };
  return (
    <div className="report-body">
      <Tiles
        items={[
          {
            label: t("Today"),
            value: fmt(view.opening),
            note: tn(view.ids.size, "in {count} budget account", "in {count} budget accounts"),
          },
          {
            label: t("Lowest point"),
            value: fmt(forecast[low]),
            tone: forecast[low] < 0 ? "negative" : undefined,
            note: low === 0 ? t("today") : formatDate(view.days[low]),
          },
          {
            label: tn(Number(ahead), "In {count} months", "In {count} months"),
            value: fmt(end),
            tone: end < 0 ? "negative" : undefined,
            note: t("{amount} from today", { amount: signed(end - view.opening) }),
          },
          { label: t("Known money in"), value: fmt(knownIn), note: t("scheduled or entered") },
          { label: t("Known money out"), value: fmt(knownOut), note: t("scheduled or entered") },
        ]}
      />
      <Panel
        title={t("Balance ahead")}
        aside={rangeControl}
      >
        <LinesChart
          keys={view.days}
          series={[
            { key: "forecast", label: t("Forecast"), color: SERIES(0), values: forecast },
            { key: "known", label: t("Known transactions only"), color: SERIES(1), values: view.knownLine, dashed: true },
          ]}
          label={tn(
            Number(ahead),
            "Forecast balance of budget accounts for the next {count} months",
            "Forecast balance of budget accounts for the next {count} months",
          )}
          format={fmt}
          height={280}
          tick={(d, i) =>
            i === 0
              ? t("Today")
              : d.endsWith("-01")
                ? (monthNames()?.[Number(d.slice(5, 7)) - 1] ??
                  new Date(`${d}T12:00:00`).toLocaleDateString("en-GB", { month: "short" }))
                : null
          }
          title={formatDate}
        />
        <Legend
          items={[
            { label: t("Forecast"), color: SERIES(0), line: true },
            { label: t("Known transactions only"), color: SERIES(1), line: true, dashed: true },
          ]}
        />
        <div className="forecast-assumption">
          <label className="field inline">
            <span>{t("Everyday spending a month")}</span>
            <input
              inputMode="decimal"
              value={draft ?? (focused ? editableAmount(monthly) : money(monthly))}
              onFocus={(e) => {
                setFocused(true);
                setDraft(editableAmount(monthly));
                e.currentTarget.select();
              }}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => {
                commit();
                setFocused(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") commit();
                if (e.key === "Escape") setDraft(null);
              }}
            />
          </label>
          {everyday !== null && everyday !== view.suggested && (
            <button className="link-button" onClick={() => setEveryday(null)}>
              {t("Use the suggested {amount}", { amount: money(view.suggested) })}
            </button>
          )}
          <p className="help">
            {t(
              "Spending that isn't scheduled, spread evenly over each day. Your last three months averaged {recent} a month; scheduled spending covers about {scheduled} a month, which suggests {suggested} for the rest.",
              {
                recent: fmt(data.recentSpending),
                scheduled: fmt(Math.round(view.scheduledSpending)),
                suggested: fmt(view.suggested),
              },
            )}
          </p>
        </div>
      </Panel>
      <Panel
        title={t("Coming up")}
        aside={<span className="muted">{tn(view.events.length, "{count} transactions", "{count} transactions")}</span>}
      >
        {view.events.length ? (
          <div className="report-table scrolls forecast-table" ref={scroller}>
            <table className="fixed" style={table.style}>
              {table.cols}
              <thead>
                <tr>
                  <th>{t("Date")}{table.grip("label", "Date")}</th>
                  <th>{t("Payee")}{table.grip("value", "Payee")}</th>
                  <th>{t("Account")}{table.grip("value", "Account")}</th>
                  <th>{t("Category")}{table.grip("value", "Category")}</th>
                  <th className="num">{t("Amount")}{table.grip("value", "amount")}</th>
                  <th className="num">{t("Balance")}{table.grip("value", "amount")}</th>
                  <th aria-hidden />
                </tr>
              </thead>
              <tbody>
                {rows.before > 0 && (
                  <tr aria-hidden className="spacer-row" style={{ height: rows.before }}>
                    <td colSpan={7} />
                  </tr>
                )}
                {view.events.slice(rows.start, rows.end).map((e, i) => {
                  const index = rows.start + i;
                  return (
                    <tr key={`${e.date}-${e.scheduleId}-${e.accountId}-${index}`}>
                      <td>{formatDate(e.date)}</td>
                      <td>
                        {e.payee || t("No payee")}
                        {!e.scheduleId && <span className="muted">{` · ${t("entered")}`}</span>}
                      </td>
                      <td>{name(e.accountId)}</td>
                      <td>{e.peerId ? t("Transfer: {account}", { account: name(e.peerId) }) : category(e.categoryId)}</td>
                      <td className={`num${e.amount < 0 ? "" : " positive"}`}>{money(e.amount)}</td>
                      <td className={`num${view.balances[index] < 0 ? " negative" : ""}`}>
                        {money(view.balances[index])}
                      </td>
                      <td aria-hidden />
                    </tr>
                  );
                })}
                {rows.after > 0 && (
                  <tr aria-hidden className="spacer-row" style={{ height: rows.after }}>
                    <td colSpan={7} />
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty-state compact">
            <p>{t("Nothing is scheduled or entered for later dates.")}</p>
            <p className="help">
              {t("Schedule your income and bills (Scheduled, in the sidebar) to make this forecast more exact.")}
            </p>
          </div>
        )}
        <p className="help">
          {t("Balances here count known transactions only.")}{" "}
          <button className="link-button" onClick={() => inspect({ accountIds: [...accounts], from: shiftDate(start, 1), to: through })}>
            {t("See entered ones in the register")}
          </button>
        </p>
      </Panel>
    </div>
  );
}
