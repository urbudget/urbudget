import { useState } from "react";
import { LoaderCircle } from "lucide-react";
import { BarList, ColumnChart, Legend, LineChart, LinesChart } from "../charts/Charts";
import { formatDate, money, monthName } from "../format";
import { cashByCurrency, holdingLine as aboutHolding } from "../holdings";
import { inNetWorth } from "../accounts";
import { today, type Account, type Portfolio } from "../types";
import { list, ltr, t, tn } from "../i18n";
import { textWidth } from "../ui/columns";
import { ASSET_SECTIONS, DEBT_SECTIONS, LIQUID, balanceSheet, heldLines, sectionTotal, type HeldLine, type SheetLine } from "./balanceSheet";
import { holdingAccounts, usePortfolio } from "./portfolio";
import {
  Panel,
  ReportTable,
  SERIES,
  SectionRow,
  Tiles,
  firstDay,
  fmt,
  lastDay,
  share,
  signed,
  sum,
  useReportColumns,
  type ReportData,
  type Scope,
  type Tile,
} from "./shared";

/** Net worth before the range starts, from the balances just before it. */
export function openingNetWorth(data: ReportData, accounts: Account[]) {
  const opening = data.openingAccounts;
  if (!opening) return data.months[0].netWorth;
  return sum(accounts.filter((a) => inNetWorth(a) && a.id in opening).map((a) => opening[a.id]));
}

/**
 * Each month's change in net worth, and where it came from: what markets did to the investment
 * accounts, and everything else (saving, debt paid down, revalued assets). The two add up to the change.
 */
export function netWorthChanges(data: ReportData, opening: number) {
  return data.months.map((m, i) => {
    const change = m.netWorth - (i ? data.months[i - 1].netWorth : opening);
    const market = m.market ?? 0;
    return { change, market, rest: change - market };
  });
}

export function NetWorth({ scope }: { scope: Scope }) {
  const { data, budget, accounts } = scope;
  const months = data.months.map((m) => m.month);
  const first = data.months[0],
    last = data.months[data.months.length - 1];
  const opening = openingNetWorth(
    data,
    budget.accounts.filter((a) => !accounts.size || accounts.has(a.id)),
  );
  const change = last.netWorth - opening;
  const changes = netWorthChanges(data, opening);
  const monthly = changes.map((c) => c.change);
  // Only a budget whose investments moved has two sources to tell apart.
  const split = changes.some((c) => c.market !== 0);
  const fromMarkets = sum(changes.map((c) => c.market));
  const shownAccounts = budget.accounts.filter((a) => !accounts.size || accounts.has(a.id));
  const table = useReportColumns("networth", 3);
  const marketTile: Tile = {
    label: t("From markets"),
    value: signed(fromMarkets),
    tone: fromMarkets < 0 ? "negative" : fromMarkets > 0 ? "positive" : undefined,
    // A share only where markets and the whole change went the same way.
    note: fromMarkets * change > 0 ? t("{share} of the change", { share: share(Math.abs(fromMarkets), Math.abs(change)) }) : undefined,
  };
  return (
    <>
      <Tiles
        items={[
          { label: t("Net worth"), value: fmt(last.netWorth), note: t("end of {month}", { month: monthName(last.month) }) },
          {
            label: t("Change"),
            value: signed(change),
            tone: change < 0 ? "negative" : "positive",
            note: `${t("since {month} began", { month: monthName(first.month) })}${opening > 0 ? ` · ${ltr(`${change >= 0 ? "+" : ""}${((change / opening) * 100).toFixed(1)}%`)}` : ""}`,
          },
          { label: t("Assets"), value: fmt(last.assets) },
          {
            label: t("Debts"),
            value: fmt(last.debt),
            note: last.assets > 0 ? t("{share} of assets", { share: share(last.debt, last.assets) }) : undefined,
          },
          ...(split ? [marketTile] : []),
        ]}
      />
      <Panel title={t("Assets, debts and net worth")}>
        <ColumnChart
          months={months}
          series={[
            { key: "assets", label: t("Assets"), color: SERIES(0), values: data.months.map((m) => m.assets) },
            { key: "debts", label: t("Debts"), color: SERIES(1), values: data.months.map((m) => -m.debt) },
          ]}
          line={{ label: t("Net worth"), color: "var(--text)", values: data.months.map((m) => m.netWorth) }}
          label={t("Assets and debts by month with net worth")}
          format={fmt}
        />
        <Legend
          items={[
            { label: t("Assets"), color: SERIES(0) },
            { label: t("Debts"), color: SERIES(1) },
            { label: t("Net worth"), color: "var(--text)", line: true },
          ]}
        />
      </Panel>
      <Panel title={t("Change each month")}>
        {split ? (
          <>
            <ColumnChart
              months={months}
              series={[
                { key: "markets", label: t("Markets"), color: SERIES(6), values: changes.map((c) => c.market) },
                { key: "rest", label: t("Everything else"), color: SERIES(0), values: changes.map((c) => c.rest) },
              ]}
              label={t("How much net worth grew or shrank each month")}
              format={fmt}
              height={200}
            />
            <Legend
              items={[
                { label: t("Markets"), color: SERIES(6) },
                { label: t("Everything else"), color: SERIES(0) },
              ]}
            />
          </>
        ) : (
          <>
            <ColumnChart
              months={months}
              series={[
                {
                  key: "up",
                  label: t("Grew"),
                  color: "var(--diverging-positive)",
                  values: monthly.map((v) => Math.max(0, v)),
                },
                {
                  key: "down",
                  label: t("Shrank"),
                  color: "var(--diverging-negative)",
                  values: monthly.map((v) => Math.min(0, v)),
                },
              ]}
              label={t("How much net worth grew or shrank each month")}
              format={fmt}
              height={200}
            />
            <Legend
              items={[
                { label: t("Grew"), color: "var(--diverging-positive)" },
                { label: t("Shrank"), color: "var(--diverging-negative)" },
              ]}
            />
          </>
        )}
      </Panel>
      <Panel title={t("By account")}>
        <ReportTable>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Account")}{table.grip("label", "Account")}</th>
                <th className="num">
                  {formatDate(firstDay(first.month))}
                  {table.grip("value", "amount")}
                </th>
                <th className="num">
                  {formatDate(lastDay(last.month))}
                  {table.grip("value", "amount")}
                </th>
                <th className="num">{t("Change")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {shownAccounts.map((a) => {
                const start = data.openingAccounts?.[a.id] ?? first.accounts[a.id] ?? 0,
                  end = last.accounts[a.id] ?? 0;
                return (
                  <tr key={a.id}>
                    <td>
                      {a.name}
                      {a.archived && <span className="muted">{` · ${t("closed")}`}</span>}
                      {!inNetWorth(a) && <span className="muted">{` · ${t("not in net worth")}`}</span>}
                    </td>
                    <td className="num">{money(start)}</td>
                    <td className="num">{money(end)}</td>
                    <td className={`num ${end - start < 0 ? "negative" : ""}`}>{money(end - start)}</td>
                    <td aria-hidden />
                  </tr>
                );
              })}
            </tbody>
            <tfoot>
              <tr>
                <th>{t("Net worth")}</th>
                <th className="num">{money(opening)}</th>
                <th className="num">{money(last.netWorth)}</th>
                <th className="num">{money(change)}</th>
                <th aria-hidden />
              </tr>
            </tfoot>
          </table>
        </ReportTable>
      </Panel>
    </>
  );
}

/**
 * What you own and owe at the start and end of the range, with the ratios that matter: assets by
 * what the money is in, debts by account type and shown as amounts owed. An account with holdings
 * is spread over the kinds it holds, its cash and its accrued income, read with the report.
 * Accounts left out of net worth are listed apart, not in the totals.
 */
export function BalanceSheet({ scope }: { scope: Scope }) {
  const { budget, accounts } = scope;
  const holds = holdingAccounts(budget).length > 0;
  const portfolio = usePortfolio(budget, { from: scope.from, to: scope.to, accountIds: [...accounts] }, holds);
  if (holds && !portfolio.data && !portfolio.error)
    return (
      <div className="empty-state compact">
        <LoaderCircle className="spin" />
      </div>
    );
  return (
    <>
      {portfolio.error && (
        <p className="form-error" role="alert">
          {portfolio.error}{" "}
          <button className="link-button" onClick={portfolio.retry}>
            {t("Retry report")}
          </button>
        </p>
      )}
      <BalanceStatement scope={scope} portfolio={portfolio.data ?? null} />
    </>
  );
}

/** The balance sheet itself; without a `portfolio` every account is one row. */
/** A section lists this many holdings or more folded, with only the first few shown. */
const FOLD_AT = 9,
  FOLDED = 6;

export function BalanceStatement({ scope, portfolio }: { scope: Scope; portfolio: Portfolio | null }) {
  const { data, budget, accounts } = scope;
  const first = data.months[0],
    last = data.months[data.months.length - 1];
  const sheet = balanceSheet(data, budget.accounts, accounts, portfolio);
  // A range that ends this month ends with what is held today: the holdings themselves are
  // listed in their sections, and cash at a broker by currency. Earlier ranges keep the accounts.
  const now = last.month === today().slice(0, 7);
  const held = now && portfolio ? heldLines(sheet, portfolio) : new Map<string, HeldLine[]>();
  const [unfolded, setUnfolded] = useState<Set<string>>(() => new Set());
  const accountName = (id: string) => budget.accounts.find((a) => a.id === id)?.name ?? "";
  // Which account holds it is said only where there is more than one to tell apart.
  const several = (portfolio?.accounts.length ?? 0) > 1;
  const holdingTitle = (r: HeldLine) => r.holding.name || r.holding.symbol;
  const holdingLine = (r: HeldLine) =>
    [...aboutHolding(r.holding, budget.currency), several ? list(r.holding.accounts.map(accountName)) : ""].filter(Boolean).join(" · ");
  const cashOf = new Map((portfolio?.accounts ?? []).map((a) => [a.accountId, a.cash]));
  const subLine = (l: SheetLine) => (now && l.part === "cash" ? cashByCurrency(cashOf.get(l.a.id), budget.currency) : null);
  // What a row is called: an account, or the part of an investment account it is.
  const rowName = (l: SheetLine) =>
    l.part === "cash"
      ? t("Cash at {name}", { name: l.a.name })
      : l.part === "accrued"
        ? t("Accrued income at {name}", { name: l.a.name })
        : l.part === "rest"
          ? t("Since prices from {date}", { date: formatDate(l.asOf!) })
          : l.a.name;
  const about = (l: SheetLine) => `${l.part === "rest" ? ` · ${l.a.name}` : ""}${l.a.archived ? ` · ${t("closed")}` : ""}`;
  // The first column starts wide enough for its longest row, indent and padding included, so
  // two accounts' rows of one kind can be told apart; it can still be dragged to any width.
  const longest = Math.max(
    0,
    ...[...sheet.assets, ...sheet.debts].map((l) => textWidth(rowName(l) + about(l), 13)),
    ...[...held.values()].flat().map((r) => textWidth(holdingTitle(r), 13)),
  );
  const table = useReportColumns("balance", 4, Math.min(380, Math.max(170, Math.ceil(longest) + 56)));
  const totals = { assets: sheet.totals.assets.end, debts: sheet.totals.debts.end };
  const net = { start: sheet.totals.assets.start - sheet.totals.debts.start, end: totals.assets - totals.debts };
  const inSection = (ls: SheetLine[], section: string) => ls.filter((l) => l.section === section);
  const liquid = sectionTotal(inSection(sheet.assets, LIQUID)).end;
  const monthlySpending = data.spending / Math.max(1, data.months.length);
  const sections = (order: string[], ls: SheetLine[]) =>
    order
      .map((section) => ({ key: section, label: t(section), value: sectionTotal(inSection(ls, section)).end }))
      .filter((s) => s.value > 0);
  // A start that isn't known, and the change from it, are left empty: the row's share says what it is.
  const amount = (v: number | null) => (v === null ? "" : money(v));
  const cells = (start: number | null, end: number, of: number | null) => (
    <>
      <td className="num">{amount(start)}</td>
      <td className="num">{money(end)}</td>
      <td className={`num ${start !== null && end - start < 0 ? "negative" : ""}`}>{amount(start === null ? null : end - start)}</td>
      <td className="num">{of === null ? "" : share(end, of)}</td>
      <td aria-hidden />
    </>
  );
  const row = (label: string, start: number, end: number, className: string) => (
    <tr className={className}>
      <td>{label}</td>
      {cells(start, end, null)}
    </tr>
  );
  const part = (title: string, order: string[], ls: SheetLine[], totalLabel: string, sums: { start: number; end: number }) => (
    <>
      <SectionRow title={title} columns={6} />
      {order
        .filter((section) => ls.some((l) => l.section === section))
        .map((section) => {
          const lines = inSection(ls, section);
          const total = sectionTotal(lines);
          // Today's holdings stand in for the accounts' lines of their kind; the rest stay.
          const holdings = held.get(section) ?? [];
          const folds = holdings.length >= FOLD_AT;
          const open = !folds || unfolded.has(section);
          const toggle = () =>
            setUnfolded((was) => {
              const next = new Set(was);
              if (next.has(section)) next.delete(section);
              else next.add(section);
              return next;
            });
          return [
            <tr key={section} className="group-line">
              <td>{t(section)}</td>
              {cells(total.start, total.end, sums.end)}
            </tr>,
            ...(open ? holdings : holdings.slice(0, FOLDED)).map((r) => (
              <tr key={r.key} className="child-line">
                <td title={[holdingTitle(r), holdingLine(r)].filter(Boolean).join(" · ")}>
                  {holdingTitle(r)}
                  {holdingLine(r) && <small className="sub-line">{holdingLine(r)}</small>}
                </td>
                {cells(null, r.holding.value, sums.end)}
              </tr>
            )),
            ...(folds
              ? [
                  <tr key={`${section}:fold`} className="child-line fold-line">
                    <td colSpan={6}>
                      <button className="link-button" onClick={toggle}>
                        {open ? t("Show fewer") : t("Show all {count}", { count: holdings.length })}
                      </button>
                    </td>
                  </tr>,
                ]
              : []),
            ...lines
              .filter((l) => !holdings.length || l.part !== "kind")
              .map((l) => (
                <tr key={l.key} className="child-line">
                  <td title={rowName(l) + about(l)}>
                    {rowName(l)}
                    {about(l) && <span className="muted">{about(l)}</span>}
                    {subLine(l) && <small className="sub-line">{subLine(l)}</small>}
                  </td>
                  {cells(l.start, l.end, sums.end)}
                </tr>
              )),
          ];
        })}
      {!ls.length && (
        <tr>
          <td className="muted" colSpan={6}>
            {t("None")}
          </td>
        </tr>
      )}
      {row(totalLabel, sums.start, sums.end, "total-row")}
    </>
  );
  const named = (some: Account[]) => list(some.map((a) => a.name));
  return (
    <>
      <Tiles
        items={[
          { label: t("Total assets"), value: fmt(totals.assets) },
          { label: t("Total liabilities"), value: fmt(totals.debts) },
          {
            label: t("Net worth"),
            value: fmt(net.end),
            tone: net.end < 0 ? "negative" : undefined,
            note: t("{amount} in this period", { amount: signed(net.end - net.start) }),
          },
          {
            label: t("Debt to assets"),
            value: totals.assets > 0 ? share(totals.debts, totals.assets) : "—",
            note: totals.debts > totals.assets ? t("you owe more than you own") : t("of what you own is owed"),
          },
          {
            label: t("Cash on hand"),
            value: fmt(liquid),
            note:
              monthlySpending > 0
                ? tn(
                    Number((liquid / monthlySpending).toFixed(1)),
                    "{count} months of spending",
                    "{count} months of spending",
                    { count: (liquid / monthlySpending).toFixed(1) },
                  )
                : t("in cash and bank accounts"),
          },
        ]}
      />
      <div className="report-pair">
        <Panel title={t("What you own")} aside={<strong>{fmt(totals.assets)}</strong>}>
          {totals.assets > 0 ? (
            <BarList
              rows={sections(ASSET_SECTIONS, sheet.assets).map((s) => ({ ...s, detail: share(s.value, totals.assets) }))}
              color={SERIES(0)}
              format={fmt}
              label={t("Assets by type")}
            />
          ) : (
            <p className="empty-state compact">{t("No assets.")}</p>
          )}
        </Panel>
        <Panel title={t("What you owe")} aside={<strong>{fmt(totals.debts)}</strong>}>
          {totals.debts > 0 ? (
            <BarList
              rows={sections(DEBT_SECTIONS, sheet.debts).map((s) => ({ ...s, detail: share(s.value, totals.debts) }))}
              color={SERIES(1)}
              format={fmt}
              label={t("Liabilities by type")}
            />
          ) : (
            <p className="empty-state compact">{t("Nothing owed.")}</p>
          )}
        </Panel>
      </div>
      <Panel
        title={t("Statement")}
        aside={
          <span className="muted">
            {t("Net worth on {date}: {amount}", { date: formatDate(lastDay(last.month)), amount: fmt(net.end) })}
          </span>
        }
      >
        <ReportTable wide>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Account")}{table.grip("label", "Account")}</th>
                <th className="num">
                  {formatDate(firstDay(first.month))}
                  {table.grip("value", "amount")}
                </th>
                <th className="num">
                  {formatDate(lastDay(last.month))}
                  {table.grip("value", "amount")}
                </th>
                <th className="num">{t("Change")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Share")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {part(t("Assets"), ASSET_SECTIONS, sheet.assets, t("Total assets"), sheet.totals.assets)}
              {sheet.investments && (
                <tr className="note-row">
                  <td colSpan={6}>
                    {t("Investments changed by {change}: {put} put in, {gain} from markets.", {
                      change: signed(sheet.investments.change),
                      put: signed(sheet.investments.put),
                      gain: signed(sheet.investments.gain),
                    })}
                  </td>
                </tr>
              )}
              {part(t("Liabilities (owed)"), DEBT_SECTIONS, sheet.debts, t("Total liabilities"), sheet.totals.debts)}
              {row(t("Net worth"), net.start, net.end, "net-row")}
              {sheet.apart.length > 0 && (
                <>
                  <SectionRow title={t("Not counted in net worth")} columns={6} />
                  {sheet.apart.map((l) => (
                    <tr key={l.a.id} className="child-line">
                      <td>
                        {l.a.name} <span className="muted">· {t(l.section).toLowerCase()}</span>
                      </td>
                      <td className="num">{money(l.start)}</td>
                      <td className="num">{money(l.end)}</td>
                      <td className="num">{money(l.end - l.start)}</td>
                      <td className="num" />
                      <td aria-hidden />
                    </tr>
                  ))}
                </>
              )}
            </tbody>
          </table>
        </ReportTable>
        <p className="help">
          {t("Balances at the start of {start} and the end of {end}. Set an account's type, or leave it out of net worth, in its settings.", {
            start: monthName(first.month),
            end: monthName(last.month),
          })}
          {sheet.crossed &&
            ` ${t("Accounts below zero count as owed and loans or cards above zero as owned, whatever their type.")}`}
        </p>
        {sheet.startUnknown.length > 0 && (
          <p className="help">
            {t("Start and change not shown by type, because what was held then isn't kept: {names}.", { names: named(sheet.startUnknown) })}
          </p>
        )}
        {sheet.unsplit.length > 0 && (
          <p className="help">
            {t("Not broken down, because what was held then isn't kept: {names}.", { names: named(sheet.unsplit) })}
          </p>
        )}
      </Panel>
    </>
  );
}

/** Each account's balance at the end of every month, a few at a time. */
export function Balances({ scope }: { scope: Scope }) {
  const { data, budget, accounts } = scope;
  const months = data.months.map((m) => m.month);
  const last = data.months[data.months.length - 1];
  const eligible = budget.accounts.filter(
    (a) =>
      (!accounts.size || accounts.has(a.id)) &&
      a.id in last.accounts &&
      (!a.archived || data.months.some((m) => m.accounts[a.id])),
  );
  // Colours follow the account: each one keeps the colour it got when it was added to the chart.
  const [chosen, setChosen] = useState<Map<string, number>>(() => {
    const biggest = [...eligible]
      .sort((a, b) => Math.abs(last.accounts[b.id] ?? 0) - Math.abs(last.accounts[a.id] ?? 0))
      .slice(0, 5);
    return new Map(eligible.filter((a) => biggest.includes(a)).map((a, i) => [a.id, i]));
  });
  const shown = eligible.filter((a) => chosen.has(a.id));
  const toggle = (id: string) =>
    setChosen((c) => {
      const next = new Map(c);
      if (next.has(id)) next.delete(id);
      else {
        const used = new Set(shown.map((a) => next.get(a.id)));
        const slot = [0, 1, 2, 3, 4, 5, 6, 7].find((s) => !used.has(s));
        if (slot === undefined) return c;
        next.set(id, slot);
      }
      return next;
    });
  const values = (id: string) => data.months.map((m) => m.accounts[id] ?? 0);
  const stats = eligible.map((a) => {
    const v = values(a.id);
    const start = data.openingAccounts?.[a.id] ?? v[0];
    return { a, start, end: v[v.length - 1], low: Math.min(...v), high: Math.max(...v) };
  });
  const mover = [...stats].sort((x, y) => Math.abs(y.end - y.start) - Math.abs(x.end - x.start))[0];
  const table = useReportColumns("balances", 5);
  return (
    <>
      <Tiles
        items={[
          {
            label: t("On the chart"),
            value: t("{count} of {total}", { count: shown.length, total: eligible.length }),
            note: t("up to {count} at a time", { count: 8 }),
          },
          { label: t("Their balance now"), value: fmt(sum(shown.map((a) => last.accounts[a.id] ?? 0))) },
          {
            label: t("Biggest change"),
            value: mover ? mover.a.name : "—",
            note: mover
              ? t("{amount} since {month} began", { amount: signed(mover.end - mover.start), month: monthName(months[0]) })
              : undefined,
          },
        ]}
      />
      <Panel title={t("Balances at the end of each month")}>
        <div className="account-chips" role="group" aria-label={t("Accounts on the chart")}>
          {eligible.map((a) => {
            const on = chosen.has(a.id);
            return (
              <button
                key={a.id}
                className={`chip-toggle${on ? " on" : ""}`}
                aria-pressed={on}
                disabled={!on && shown.length >= 8}
                onClick={() => toggle(a.id)}
              >
                <i className="key-box" style={{ background: on ? SERIES(chosen.get(a.id)!) : "transparent" }} />
                {a.name}
              </button>
            );
          })}
        </div>
        {shown.length ? (
          <>
            <LinesChart
              keys={months}
              series={shown.map((a) => ({
                key: a.id,
                label: a.name,
                color: SERIES(chosen.get(a.id)!),
                values: values(a.id),
              }))}
              label={t("Balance of each chosen account at the end of each month")}
              format={fmt}
              height={280}
            />
            <Legend items={shown.map((a) => ({ label: a.name, color: SERIES(chosen.get(a.id)!), line: true }))} />
          </>
        ) : (
          <p className="empty-state compact">{t("Choose accounts above to chart them.")}</p>
        )}
      </Panel>
      <Panel title={t("All accounts")}>
        <ReportTable>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Account")}{table.grip("label", "Account")}</th>
                <th className="num">{t("Start")}{table.grip("value", "amount")}</th>
                <th className="num">{t("End")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Change")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Lowest")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Highest")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {stats.map((s) => (
                <tr key={s.a.id}>
                  <td>
                    {s.a.name}
                    {s.a.archived && <span className="muted">{` · ${t("closed")}`}</span>}
                  </td>
                  <td className="num">{money(s.start)}</td>
                  <td className="num">{money(s.end)}</td>
                  <td className={`num${s.end - s.start < 0 ? " negative" : ""}`}>{money(s.end - s.start)}</td>
                  <td className="num">{money(s.low)}</td>
                  <td className="num">{money(s.high)}</td>
                  <td aria-hidden />
                </tr>
              ))}
            </tbody>
          </table>
        </ReportTable>
        <p className="help">{t("Month-end balances; lowest and highest are among those.")}</p>
      </Panel>
    </>
  );
}

export function AgeOfMoney({ scope }: { scope: Scope }) {
  const { data } = scope;
  const months = data.months.map((m) => m.month);
  const values = months.map((m) => data.ageOfMoneyByMonth?.[m] ?? null);
  const known = values.filter((v): v is number => v !== null);
  const firstKnown = known[0];
  const table = useReportColumns("age", 1);
  const days = (v: number | null | undefined) =>
    v === null || v === undefined ? "—" : tn(v, "{count} day", "{count} days");
  const change = known.length > 1 ? known[known.length - 1] - firstKnown : 0;
  return (
    <>
      <Tiles
        items={[
          { label: t("Cash age"), value: days(data.ageOfMoney) },
          {
            label: t("Change"),
            value:
              known.length > 1
                ? tn(Math.abs(change), "{change} days", "{change} days", { change: `${change >= 0 ? "+" : ""}${change}` })
                : "—",
            tone: known.length > 1 ? (known[known.length - 1] >= firstKnown ? "positive" : "negative") : undefined,
            note: known.length > 1 ? t("over these months") : undefined,
          },
          { label: t("Highest"), value: known.length ? days(Math.max(...known)) : "—" },
          { label: t("Lowest"), value: known.length ? days(Math.min(...known)) : "—" },
        ]}
      />
      <Panel title={t("Cash age at the end of each month")}>
        <LineChart
          months={months}
          values={values}
          color={SERIES(0)}
          label={t("Cash age at the end of each month, in days")}
          seriesLabel={t("Cash age")}
          format={(v) => tn(v, "{count} days", "{count} days")}
          axisFormat={(v) => t("{count}d", { count: v })}
        />
        <p className="help">
          {t("How long money sits in your cash accounts before you spend it: the average age of the last ten cash outflows, first in, first out, to the nearest day. Until there are ten, the missing ones count as zero days. It is an estimate when opening balances have no known dates.")}
        </p>
        <ReportTable>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Month")}{table.grip("label", "Month")}</th>
                <th className="num">{t("Days")}{table.grip("value", "Days")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {months.map((m, i) => (
                <tr key={m}>
                  <td>{monthName(m, "short")}</td>
                  <td className="num">{values[i] ?? "—"}</td>
                  <td aria-hidden />
                </tr>
              ))}
            </tbody>
          </table>
        </ReportTable>
      </Panel>
    </>
  );
}
