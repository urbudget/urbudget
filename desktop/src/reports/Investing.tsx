import { Fragment, useState, type ReactNode } from "react";
import { LoaderCircle } from "lucide-react";
import { daysAgo } from "../BankSync";
import { BarList, ColumnChart, Donut, Legend, type Series } from "../charts/Charts";
import { formatDate, money, monthName, percent } from "../format";
import { holdingLine, kindName, ownAmount, sourceName } from "../holdings";
import { lackOf, staleHoldings } from "../Investments";
import { Tabs } from "../ui";
import { list, ltr, msg, t, tn } from "../i18n";
import type { Budget, Portfolio, PortfolioAccount, PortfolioPart } from "../types";
import { usePortfolio } from "./portfolio";
import { OTHER, Panel, ReportTable, SERIES, Tiles, fmt, headingWidth, ranked, signed, sum, useReportColumns } from "./shared";

/** The three reports on investments: how they did, what they hold, and what they earned and cost. */
export type InvestmentsReport = "performance" | "allocation" | "earnings";
/** Opens an account's own page, where its Holdings view is. */
type OpenAccount = (accountId: string) => void;
type Body = { portfolio: Portfolio; budget: Budget; onOpenAccount?: OpenAccount };

/** A rate of return with its sign ("+5.4%"), or a dash where there was no money at work to measure against. */
const rate = (r: number | null) => (r === null ? "—" : ltr(percent(r, true)));
/** A share of a whole to a tenth of a percent, as the other reports write theirs. */
const part = (s: number | null) => (s === null ? "—" : ltr(`${(s * 100).toFixed(1)}%`));
const toned = (n: number) => (n < 0 ? "negative" : n > 0 ? "positive" : undefined);
const nameIn = (budget: Budget) => (id: string) => budget.accounts.find((a) => a.id === id)?.name ?? "";
/** What to say of prices that have aged: a broker's after a few days, a file's or a typed value's after a month. */
const aged = (a: PortfolioAccount) =>
  a.source === "ibkr" || a.source === "simplefin"
    ? staleHoldings(a)
      ? t("These prices are a few days old.")
      : ""
    : a.asOf < daysAgo(35)
      ? t("That is over a month ago: update it to keep the gain current.")
      : "";

/**
 * The reports on investments: the accounts with holdings taken together. They read their own
 * figures, as the forecast does, for the months and accounts the filters choose.
 */
export function Investing({
  report,
  budget,
  filter,
  onOpenAccount,
}: {
  report: InvestmentsReport;
  budget: Budget;
  filter: { from: string; to: string; accountIds: string[] };
  onOpenAccount?: OpenAccount;
}) {
  const { data, error, retry } = usePortfolio(budget, filter);
  if (!data)
    return (
      <div className="report-body">
        {error ? (
          <p className="form-error" role="alert">
            {error}{" "}
            <button className="link-button" onClick={retry}>
              {t("Retry report")}
            </button>
          </p>
        ) : (
          <div className="empty-state compact">
            <LoaderCircle className="spin" />
          </div>
        )}
      </div>
    );
  const body = { portfolio: data, budget, onOpenAccount };
  return (
    <div className="report-body" key={report}>
      {report === "performance" ? <Performance {...body} /> : report === "allocation" ? <Allocation {...body} /> : <Earnings {...body} />}
    </div>
  );
}

/** Accounts by name in a sentence, each a way to its page when there is one. */
function Names({ ids, budget, onOpenAccount }: { ids: string[]; budget: Budget; onOpenAccount?: OpenAccount }) {
  const name = nameIn(budget);
  if (!onOpenAccount) return <>{list(ids.map(name))}</>;
  const comma = list(["", ""]);
  return (
    <>
      {ids.map((id, i) => (
        <Fragment key={id}>
          {i > 0 && comma}
          <button className="link-button" onClick={() => onOpenAccount(id)}>
            {name(id)}
          </button>
        </Fragment>
      ))}
    </>
  );
}

/**
 * How the investments did over the range: what they are worth, what was put in, and what they
 * gained beyond that, month by month and account by account.
 */
export function Performance({ portfolio: p, budget, onOpenAccount }: Body) {
  const table = useReportColumns("performance", 6, 190);
  const name = nameIn(budget);
  const { total } = p;
  const months = p.months.map((m) => m.month);
  // The six largest accounts have a colour each; the rest share one.
  const bySize = [...p.accounts].sort((a, b) => b.end - a.end);
  const rest = bySize.slice(6);
  const series: Series[] = [
    ...bySize.slice(0, 6).map((a, i) => ({
      key: a.accountId,
      label: name(a.accountId),
      color: SERIES(i),
      values: p.months.map((m) => m.byAccount[a.accountId] ?? 0),
    })),
    ...(rest.length
      ? [{ key: "__other", label: t("Other"), color: OTHER, values: p.months.map((m) => sum(rest.map((a) => m.byAccount[a.accountId] ?? 0))) }]
      : []),
  ];
  // Money put in by each month's end: the value less every gain so far, so the room between the
  // columns and the line is the gain.
  let gained = 0;
  const putIn = p.months.map((m) => m.value - (gained += m.gain));
  const estimated = sum(p.accounts.map((a) => a.estimatedDays));
  const lacking = [...new Set(p.accounts.flatMap((a) => a.missing))].map(lackOf).filter((text): text is string => !!text);
  const sparse = p.accounts.filter((a) => a.sparse).map((a) => name(a.accountId));
  const old = p.accounts.filter((a) => aged(a));
  return (
    <>
      <Tiles
        items={[
          { label: t("Value now"), value: fmt(total.end), note: p.to ? t("end of {month}", { month: monthName(p.to) }) : undefined },
          {
            label: t("Gain"),
            value: signed(total.gain),
            tone: toned(total.gain),
            note: [
              p.from ? t("since {month} began", { month: monthName(p.from) }) : "",
              total.realised === null ? "" : t("Realised {amount}", { amount: signed(total.realised) }),
            ]
              .filter(Boolean)
              .join(" · "),
          },
          {
            label: t("Return"),
            value: rate(total.rate),
            tone: toned(total.rate ?? 0),
            note: total.yearly === null ? undefined : t("{rate} a year", { rate: rate(total.yearly) }),
          },
          { label: t("Money put in"), value: signed(total.put) },
        ]}
      />
      <Panel title={t("Value")}>
        <ColumnChart
          months={months}
          series={series}
          line={{ label: t("Money put in"), color: "var(--text)", values: putIn }}
          label={t("Value")}
          format={fmt}
          height={260}
        />
        <Legend
          items={[
            ...series.map((s) => ({ label: s.label, color: s.color })),
            { label: t("Money put in"), color: "var(--text)", line: true },
          ]}
        />
      </Panel>
      <Panel title={t("Gain each month")}>
        <ColumnChart
          months={months}
          series={[
            { key: "gain", label: t("Gain"), color: "var(--positive)", values: p.months.map((m) => Math.max(0, m.gain)) },
            { key: "loss", label: t("Loss"), color: "var(--negative)", values: p.months.map((m) => Math.min(0, m.gain)) },
          ]}
          label={t("Gain each month")}
          format={(v) => `${v > 0 ? "+" : ""}${fmt(v)}`}
          height={200}
          total={false}
        />
        {sparse.length > 0 && (
          <p className="help report-note">
            {t("Valued only when updated, so a gain shows in the month of the next update: {names}.", { names: list(sparse) })}
          </p>
        )}
      </Panel>
      <Panel title={t("By account")}>
        <ReportTable wide>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Account")}{table.grip("label", "Account")}</th>
                <th>{t("Kept up to date by")}{table.grip("value", "source")}</th>
                <th className="num">{t("Start")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Put in")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Gain")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Return")}{table.grip("value", "amount")}</th>
                <th className="num">{t("End")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {p.accounts.map((a) => (
                <tr key={a.accountId}>
                  <td>
                    {onOpenAccount ? (
                      <button className="link-button cell-link" onClick={() => onOpenAccount(a.accountId)} title={t("Holdings")}>
                        {name(a.accountId)}
                      </button>
                    ) : (
                      name(a.accountId)
                    )}
                  </td>
                  <td>{sourceName(a)}</td>
                  <td className="num">{money(a.start)}</td>
                  <td className="num">{signed(a.put)}</td>
                  <td className={`num ${toned(a.gain) ?? ""}`}>{signed(a.gain)}</td>
                  <td className={`num ${toned(a.rate ?? 0) ?? ""}`}>{rate(a.rate)}</td>
                  <td className="num">{money(a.end)}</td>
                  <td aria-hidden />
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <th>{t("Total")}</th>
                <th />
                <th className="num">{money(total.start)}</th>
                <th className="num">{signed(total.put)}</th>
                <th className="num">{signed(total.gain)}</th>
                <th className="num">{rate(total.rate)}</th>
                <th className="num">{money(total.end)}</th>
                <th aria-hidden />
              </tr>
            </tfoot>
          </table>
        </ReportTable>
        {estimated > 0 && (
          <p className="help report-note">
            {tn(
              estimated,
              "On {count} day the value jumped with no deposit or transfer listed, so that change is counted as money moved, not as a gain. Add the Transfers section to your Flex Query for exact figures.",
              "On {count} days the value jumped with no deposit or transfer listed, so those changes are counted as money moved, not as gains. Add the Transfers section to your Flex Query for exact figures.",
            )}
          </p>
        )}
        {lacking.length > 0 && (
          <p className="help report-note">
            {t("Your Flex Query leaves some sections out, so parts of this page are missing:")} {lacking.join(" ")}
          </p>
        )}
        {old.map((a) => (
          <p className="help report-note" key={a.accountId}>
            {name(a.accountId)} · {t("Prices from")} {formatDate(a.asOf)} · {aged(a)}
          </p>
        ))}
        {p.unfilled.length > 0 && (
          <p className="help report-note">
            {t("Not counted here, because nothing keeps them up to date: {names}.", {
              names: <Names ids={p.unfilled} budget={budget} onOpenAccount={onOpenAccount} />,
            })}
          </p>
        )}
      </Panel>
    </>
  );
}

type Split = "kind" | "account" | "currency";
/** The engine's names for holdings a source gave no kind for; here they go by the Balance sheet's name for them. */
const UNKINDED = ["Holdings", "Other holdings"];
const OTHER_INVESTMENTS = msg("Other investments");
/** The kinds with the unkinded holdings as one part, largest first. */
function kindsOf(byKind: PortfolioPart[]) {
  const rest = byKind.filter((k) => UNKINDED.includes(k.key));
  const kinds = byKind.filter((k) => !UNKINDED.includes(k.key));
  if (rest.length) kinds.push({ key: OTHER_INVESTMENTS, value: sum(rest.map((k) => k.value)), share: sum(rest.map((k) => k.share)) });
  return kinds.sort((x, y) => y.value - x.value);
}
/** How many of the merged holdings show before "Show all". */
const FIRST = 10;

/** What the investments are in today, across the accounts: by kind, account or currency, and the largest holdings. */
export function Allocation({ portfolio: p, budget }: Body) {
  const [by, setBy] = useState<Split>("kind");
  const [all, setAll] = useState(false);
  const table = useReportColumns("allocation", 5, 230);
  const name = nameIn(budget);
  const a = p.allocation;
  // Cash and accrued income are among the kinds.
  const parts = by === "kind" ? kindsOf(a.byKind) : by === "account" ? a.byAccount : a.byCurrency;
  const label = (key: string) => (by === "kind" ? (key === OTHER_INVESTMENTS ? t(OTHER_INVESTMENTS) : kindName(key)) : by === "account" ? name(key) : key);
  const { coloured, folded } = ranked(parts.map((x) => ({ ...x, label: label(x.key) })));
  const largest = p.holdings[0];
  const shown = all ? p.holdings : p.holdings.slice(0, FIRST);
  const cashShare = a.total > 0 ? Math.max(0, a.cash) / a.total : null;
  return (
    <>
      <Tiles
        items={[
          { label: t("Holdings"), value: a.count, note: fmt(a.total) },
          { label: t("Largest holding"), value: part(a.largestShare), note: largest ? largest.name || largest.symbol : undefined },
          { label: t("Top five holdings"), value: part(a.topFiveShare) },
          { label: t("Cash"), value: part(cashShare), note: fmt(a.cash) },
        ]}
      />
      <Panel
        title={t("What it holds")}
        aside={
          <Tabs<Split>
            label={t("Show")}
            value={by}
            onChange={setBy}
            tabs={[
              { id: "kind", label: t("By kind") },
              { id: "account", label: t("By account") },
              { id: "currency", label: t("By currency") },
            ]}
          />
        }
      >
        {coloured.length ? (
          <div className="donut-layout">
            <Donut
              slices={folded.filter((x) => x.value > 0)}
              label={by === "kind" ? t("By kind") : by === "account" ? t("By account") : t("By currency")}
              format={fmt}
              center={{ value: money(a.total), label: t("Value") }}
            />
            <BarList
              rows={coloured.map((x) => ({
                key: x.key,
                label: x.label,
                // A currency's own amount, under it, where it isn't the budget's.
                sub: by === "currency" ? (ownAmount(x.amount, x.key, budget.currency) ?? undefined) : undefined,
                value: x.value,
                color: x.color,
                detail: x.value > 0 ? part(x.share) : "",
              }))}
              color={SERIES(0)}
              format={fmt}
              label={by === "kind" ? t("By kind") : by === "account" ? t("By account") : t("By currency")}
            />
          </div>
        ) : (
          <p className="empty-state compact">{t("None")}</p>
        )}
        <p className="help report-note">{t("Today's holdings. What was held on past days isn't kept.")}</p>
        {p.accounts.map((x) => (
          <p className="help report-note" key={x.accountId}>
            {name(x.accountId)} · {t("Prices from")} {formatDate(x.asOf)}
            {aged(x) && ` · ${aged(x)}`}
          </p>
        ))}
      </Panel>
      {p.holdings.length > 0 && (
        <Panel title={t("Largest holdings")} aside={<span className="muted">{tn(p.holdings.length, "{count} holding", "{count} holdings")}</span>}>
          <ReportTable wide>
            <table className="fixed" style={table.style}>
              {table.cols}
              <thead>
                <tr>
                  <th>{t("Holding")}{table.grip("label", "Holding")}</th>
                  <th>{t("Accounts")}{table.grip("value", "Accounts")}</th>
                  <th className="num">{t("Value")}{table.grip("value", "amount")}</th>
                  <th className="num">{t("Share")}{table.grip("value", "amount")}</th>
                  <th className="num">{t("Unrealised gain")}{table.grip("value", "amount")}</th>
                  <th className="num">{t("Return on cost")}{table.grip("value", "amount")}</th>
                  <th aria-hidden />
                </tr>
              </thead>
              <tbody>
                {shown.map((h) => {
                  // Under its name: what it is, its symbol where that adds something, and what it is
                  // worth in its own currency where that isn't the budget's.
                  const about = holdingLine(h, budget.currency).join(" · ");
                  return (
                    <tr key={h.key}>
                      <td title={[h.name || h.symbol, about].filter(Boolean).join(" · ")}>
                        {h.name || h.symbol}
                        {about && <small className="sub-line">{about}</small>}
                      </td>
                      <td>{list(h.accounts.map(name))}</td>
                      <td className="num">{money(h.value)}</td>
                      <td className="num">{h.value > 0 ? part(h.share) : ""}</td>
                      <td className={`num ${toned(h.gain ?? 0) ?? ""}`}>{h.gain === null ? "" : signed(h.gain)}</td>
                      <td className={`num ${toned(h.gain ?? 0) ?? ""}`}>{h.rate === null ? "" : rate(h.rate)}</td>
                      <td aria-hidden />
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </ReportTable>
          {p.holdings.length > FIRST && (
            <p className="help report-note">
              <button className="link-button" onClick={() => setAll(!all)}>
                {all ? t("Show fewer") : t("Show all {count}", { count: p.holdings.length })}
              </button>
            </p>
          )}
        </Panel>
      )}
    </>
  );
}

/** What the investments paid and what they cost to hold: dividends and interest against tax, fees and commissions. */
export function Earnings({ portfolio: p, budget }: Body) {
  const inc = p.income;
  // Amounts that are none of the four have a column only where there are any.
  const other = inc.months.some((m) => m.other !== 0);
  // Commissions have a series and a column where a statement gives them by month.
  const monthly = inc.months.some((m) => m.commissions !== null);
  // That column's heading is longer than its amounts, so it sets the columns' width.
  const months = useReportColumns(
    "earnings-months",
    5 + (other ? 1 : 0) + (monthly ? 1 : 0),
    undefined,
    monthly ? Math.max(112, headingWidth(t("Commissions on trades"))) : undefined,
  );
  const holdings = useReportColumns("earnings-holdings", 2, 230);
  const [all, setAll] = useState(false);
  // Its headings are longer than its amounts, so they set the columns' width.
  const accounts = useReportColumns(
    "earnings-accounts",
    3,
    190,
    Math.max(112, headingWidth(t("Dividends and interest")), headingWidth(t("Fees, tax and commissions"))),
  );
  const name = nameIn(budget);
  // By month where they are known that way, and a statement's one figure for the other accounts.
  const byMonth = sum(inc.months.map((m) => m.commissions ?? 0));
  const commissions = byMonth + sum(inc.commissions.map((c) => c.amount));
  const earned = inc.dividends + inc.interest;
  const costs = inc.tax + inc.fees + commissions;
  const net = earned + costs + inc.other;
  const keys = inc.months.map((m) => m.month);
  const series: Series[] = [
    { key: "dividends", label: t("Dividends"), color: SERIES(0), values: inc.months.map((m) => m.dividends) },
    { key: "interest", label: t("Interest"), color: SERIES(1), values: inc.months.map((m) => m.interest) },
    { key: "tax", label: t("Tax withheld"), color: SERIES(2), values: inc.months.map((m) => m.tax) },
    { key: "fees", label: t("Fees"), color: SERIES(3), values: inc.months.map((m) => m.fees) },
    ...(monthly ? [{ key: "commissions", label: t("Commissions on trades"), color: SERIES(4), values: inc.months.map((m) => m.commissions ?? 0) }] : []),
    ...(other ? [{ key: "other", label: t("Other"), color: OTHER, values: inc.months.map((m) => m.other) }] : []),
  ].filter((s) => s.values.some((v) => v !== 0));
  // Each account over the range, its trades' commissions among its costs.
  const rows = inc.byAccount.map((a) => {
    const earned = a.dividends + a.interest;
    const costs = a.tax + a.fees + (a.commissions ?? 0);
    return { id: a.accountId, earned, costs, net: earned + costs + a.other };
  });
  const cell = (n: number): ReactNode => (n === 0 ? money(0) : signed(n));
  const shownHoldings = all ? inc.byHolding : inc.byHolding.slice(0, FIRST);
  return (
    <>
      <Tiles
        items={[
          { label: t("Dividends and interest"), value: signed(earned), tone: toned(earned) },
          {
            label: t("Fees, tax and commissions"),
            value: signed(costs),
            tone: costs < 0 ? "negative" : undefined,
            // All of them: the ones in the months' table, and the statements' own figures, which aren't.
            note: monthly || inc.commissions.length ? `${t("Commissions on trades")} ${signed(commissions)}` : undefined,
          },
          { label: t("Net"), value: signed(net), tone: toned(net) },
          { label: t("Yield"), value: part(inc.yield), note: t("Income over the average value") },
        ]}
      />
      {inc.months.length > 0 && (
        <Panel title={t("By month")}>
          <ColumnChart months={keys} series={series} label={t("Income and costs")} format={(v) => `${v > 0 ? "+" : ""}${fmt(v)}`} height={220} />
          <Legend items={series.map((s) => ({ label: s.label, color: s.color }))} />
          <ReportTable>
            <table className="fixed" style={months.style}>
              {months.cols}
              <thead>
                <tr>
                  <th>{t("Month")}{months.grip("label", "Month")}</th>
                  <th className="num">{t("Dividends")}{months.grip("value", "amount")}</th>
                  <th className="num">{t("Interest")}{months.grip("value", "amount")}</th>
                  <th className="num">{t("Tax withheld")}{months.grip("value", "amount")}</th>
                  <th className="num">{t("Fees")}{months.grip("value", "amount")}</th>
                  {other && <th className="num">{t("Other")}{months.grip("value", "amount")}</th>}
                  {monthly && <th className="num">{t("Commissions on trades")}{months.grip("value", "amount")}</th>}
                  <th className="num">{t("Net")}{months.grip("value", "amount")}</th>
                  <th aria-hidden />
                </tr>
              </thead>
              <tbody>
                {inc.months.map((m) => {
                  const sumOf = m.dividends + m.interest + m.tax + m.fees + m.other + (m.commissions ?? 0);
                  // A month with nothing in it is on the chart, not in the table.
                  if (![m.dividends, m.interest, m.tax, m.fees, m.other, m.commissions].some(Boolean)) return null;
                  return (
                    <tr key={m.month}>
                      <td>{monthName(m.month, "short")}</td>
                      <td className="num">{cell(m.dividends)}</td>
                      <td className="num">{cell(m.interest)}</td>
                      <td className="num">{cell(m.tax)}</td>
                      <td className="num">{cell(m.fees)}</td>
                      {other && <td className="num">{cell(m.other)}</td>}
                      {monthly && <td className="num">{m.commissions === null ? "—" : cell(m.commissions)}</td>}
                      <td className={`num ${toned(sumOf) ?? ""}`}>{cell(sumOf)}</td>
                      <td aria-hidden />
                    </tr>
                  );
                })}
              </tbody>
              <tfoot>
                <tr>
                  <th>{t("Total")}</th>
                  <th className="num">{cell(inc.dividends)}</th>
                  <th className="num">{cell(inc.interest)}</th>
                  <th className="num">{cell(inc.tax)}</th>
                  <th className="num">{cell(inc.fees)}</th>
                  {other && <th className="num">{cell(inc.other)}</th>}
                  {monthly && <th className="num">{cell(byMonth)}</th>}
                  <th className="num">{cell(inc.dividends + inc.interest + inc.tax + inc.fees + inc.other + byMonth)}</th>
                  <th aria-hidden />
                </tr>
              </tfoot>
            </table>
          </ReportTable>
        </Panel>
      )}
      {rows.length + inc.commissions.length + inc.silent.length > 0 && (
      <Panel title={t("By account")}>
        {rows.length > 0 && (
          <ReportTable>
            <table className="fixed" style={accounts.style}>
              {accounts.cols}
              <thead>
                <tr>
                  <th>{t("Account")}{accounts.grip("label", "Account")}</th>
                  <th className="num">{t("Dividends and interest")}{accounts.grip("value", "amount")}</th>
                  <th className="num">{t("Fees, tax and commissions")}{accounts.grip("value", "amount")}</th>
                  <th className="num">{t("Net")}{accounts.grip("value", "amount")}</th>
                  <th aria-hidden />
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={r.id}>
                    <td>{name(r.id)}</td>
                    <td className="num">{cell(r.earned)}</td>
                    <td className="num">{cell(r.costs)}</td>
                    <td className={`num ${toned(r.net) ?? ""}`}>{cell(r.net)}</td>
                    <td aria-hidden />
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <th>{t("Total")}</th>
                  <th className="num">{cell(earned)}</th>
                  <th className="num">{cell(costs)}</th>
                  <th className="num">{cell(net)}</th>
                  <th aria-hidden />
                </tr>
              </tfoot>
            </table>
          </ReportTable>
        )}
        {inc.commissions.map((c) => (
          <p className="help report-note" key={`${c.accountId}:${c.from}`}>
            {name(c.accountId)} · {t("Commissions on trades")} {signed(c.amount)} ·{" "}
            {t("{from} to {to}", { from: formatDate(c.from), to: formatDate(c.to) })}
          </p>
        ))}
        {inc.silent.length > 0 && <p className="help report-note">{t("No income or costs reported: {names}.", { names: list(inc.silent.map(name)) })}</p>}
      </Panel>
      )}
      {inc.byHolding.length > 0 && (
        <Panel title={t("By holding")}>
          <ReportTable>
            <table className="fixed" style={holdings.style}>
              {holdings.cols}
              <thead>
                <tr>
                  <th>{t("Holding")}{holdings.grip("label", "Holding")}</th>
                  <th className="num">{t("Dividends")}{holdings.grip("value", "amount")}</th>
                  <th className="num">{t("Tax withheld")}{holdings.grip("value", "amount")}</th>
                  <th aria-hidden />
                </tr>
              </thead>
              <tbody>
                {shownHoldings.map((h) => (
                  <tr key={h.key}>
                    <td title={h.name || h.symbol}>
                      {h.name || h.symbol}
                      {h.name && h.name !== h.symbol && <span className="muted">{` · ${h.symbol}`}</span>}
                    </td>
                    <td className="num">{cell(h.dividends)}</td>
                    <td className="num">{cell(h.tax)}</td>
                    <td aria-hidden />
                  </tr>
                ))}
              </tbody>
            </table>
          </ReportTable>
          {inc.byHolding.length > FIRST && (
            <p className="help report-note">
              <button className="link-button" onClick={() => setAll(!all)}>
                {all ? t("Show fewer") : t("Show all {count}", { count: inc.byHolding.length })}
              </button>
            </p>
          )}
        </Panel>
      )}
    </>
  );
}
