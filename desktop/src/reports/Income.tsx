import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { BarList, ColumnChart, Donut, Legend, LinesChart } from "../charts/Charts";
import { money, monthName } from "../format";
import { ltr, t, tn } from "../i18n";
import {
  Panel,
  ReportTable,
  SERIES,
  SectionRow,
  Tiles,
  fmt,
  ranked,
  share,
  signed,
  sum,
  useReportColumns,
  type Scope,
} from "./shared";

const percent = (v: number) => ltr(`${v}%`);

/** Income against spending at the budget boundary, including categorized tracking transfers. */
export function IncomeExpense({ scope }: { scope: Scope }) {
  const { data, groupOf, nameOf, spendingGroups } = scope;
  const [open, setOpen] = useState<Set<string>>(new Set());
  const months = data.months.map((m) => m.month);
  const n = Math.max(1, months.length);
  const net = data.months.map((m) => m.netIncome);
  const kept = data.income - data.spending;
  const rates = data.months.map((m) => (m.income > 0 ? Math.round((m.netIncome / m.income) * 1000) / 10 : null));
  let running = 0;
  const cumulative = net.map((v) => (running += v));
  const savedMonths = net.filter((v) => v > 0).length;
  const best = data.months.reduce<(typeof data.months)[number] | null>(
    (b, m) => (!b || m.netIncome > b.netIncome ? m : b),
    null,
  );
  const sources = Object.entries(data.incomeSources).sort((a, b) => b[1] - a[1]);
  const groupRows = spendingGroups
    .map((g) => {
      const ids = Object.keys(data.categories).filter((id) => groupOf.get(id) === g);
      return {
        group: g,
        values: data.months.map((m) => sum(ids.map((id) => m.categories[id] ?? 0))),
        categories: ids
          .map((id) => ({ id, values: data.months.map((m) => m.categories[id] ?? 0) }))
          .sort((a, b) => sum(b.values) - sum(a.values)),
      };
    })
    .filter((g) => g.values.some((v) => v !== 0));
  const table = useReportColumns("income-expense", months.length + 2);
  const cells = (values: number[], strong = false) => (
    <>
      {values.map((v, i) => (
        <td key={months[i]} className={`num${v < 0 && strong ? " negative" : ""}`}>
          {strong ? <strong>{money(v)}</strong> : money(v)}
        </td>
      ))}
      <td className="num">
        <strong>{money(sum(values))}</strong>
      </td>
      <td className="num">{money(Math.round(sum(values) / n))}</td>
      <td aria-hidden />
    </>
  );
  return (
    <>
      <Tiles
        items={[
          { label: t("Income"), value: fmt(data.income), note: t("{amount} a month", { amount: fmt(Math.round(data.income / n)) }) },
          { label: t("Spending"), value: fmt(data.spending), note: t("{amount} a month", { amount: fmt(Math.round(data.spending / n)) }) },
          {
            label: t("Budget surplus"),
            value: fmt(kept),
            tone: kept < 0 ? "negative" : "positive",
            note: t("{amount} a month", { amount: signed(Math.round(kept / n)) }),
          },
          {
            label: t("Budget surplus rate"),
            value: data.income > 0 ? ltr(`${((kept / data.income) * 100).toFixed(1)}%`) : "—",
            note: t("surplus as a share of income"),
          },
          {
            label: t("Positive-surplus months"),
            value: t("{count} of {total}", { count: savedMonths, total: data.months.length }),
            note: best && best.netIncome > 0 ? t("best: {month}", { month: monthName(best.month) }) : undefined,
          },
        ]}
      />
      <p className="help">
        {t("Budget surplus compares income with spending. Categorized transfers to tracking investments and loans reduce it; use Net worth to see changes in wealth.")}
        {scope.someCategories && ` ${t("Income stays included; spending reflects the chosen categories.")}`}
      </p>
      <Panel title={t("Income and spending by month")}>
        <ColumnChart
          months={months}
          grouped
          series={[
            { key: "income", label: t("Income"), color: SERIES(0), values: data.months.map((m) => m.income) },
            { key: "spending", label: t("Spending"), color: SERIES(1), values: data.months.map((m) => m.spending) },
          ]}
          line={{ label: t("Budget surplus"), color: "var(--text)", values: net }}
          label={t("Income and spending side by side each month, with budget surplus as a line")}
          format={fmt}
        />
        <Legend
          items={[
            { label: t("Income"), color: SERIES(0) },
            { label: t("Spending"), color: SERIES(1) },
            { label: t("Budget surplus"), color: "var(--text)", line: true },
          ]}
        />
      </Panel>
      <div className="report-pair">
        <Panel title={t("Budget surplus rate")} aside={<span className="muted">{t("Surplus as a share of income")}</span>}>
          <LinesChart
            keys={months}
            series={[{ key: "rate", label: t("Budget surplus rate"), color: SERIES(0), values: rates }]}
            label={t("Budget surplus each month, as a percentage of income")}
            format={percent}
            axisFormat={percent}
            height={200}
          />
        </Panel>
        <Panel title={t("Cumulative budget surplus")} aside={<span className="muted">{t("Surplus added up")}</span>}>
          <LinesChart
            keys={months}
            series={[{ key: "surplus", label: t("Cumulative budget surplus"), color: SERIES(2), values: cumulative }]}
            label={t("Budget surplus added up month by month")}
            format={fmt}
            height={200}
          />
        </Panel>
      </div>
      <Panel title={t("Income and spending")} aside={<span className="muted">{t("Select a group to see its categories")}</span>}>
        <ReportTable wide>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Source or group")}{table.grip("label", "Name")}</th>
                {months.map((m) => (
                  <th key={m} className="num">
                    {monthName(m, "short")}
                    {table.grip("value", "amount")}
                  </th>
                ))}
                <th className="num">{t("Total")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Average")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              <SectionRow title={t("Income")} columns={months.length + 4} />
              {sources.map(([payee]) => (
                <tr key={`in-${payee}`}>
                  <td>{payee || t("No payee")}</td>
                  {cells(data.months.map((m) => m.incomeSources[payee] ?? 0))}
                </tr>
              ))}
              <tr className="total-row">
                <td>{t("Total income")}</td>
                {cells(
                  data.months.map((m) => m.income),
                  true,
                )}
              </tr>
              <SectionRow title={t("Spending")} columns={months.length + 4} />
              {groupRows.map((g) => {
                const expanded = open.has(g.group);
                return [
                  <tr key={`g-${g.group}`} className="group-line">
                    <td>
                      <button
                        className="link-button"
                        aria-expanded={expanded}
                        onClick={() =>
                          setOpen((s) => {
                            const next = new Set(s);
                            if (!next.delete(g.group)) next.add(g.group);
                            return next;
                          })
                        }
                      >
                        {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />} {g.group}
                      </button>
                    </td>
                    {cells(g.values)}
                  </tr>,
                  ...(expanded
                    ? g.categories.map((c) => (
                        <tr key={`c-${c.id}`} className="child-line">
                          <td>{nameOf.get(c.id) ?? c.id}</td>
                          {cells(c.values)}
                        </tr>
                      ))
                    : []),
                ];
              })}
              <tr className="total-row">
                <td>{t("Total spending")}</td>
                {cells(
                  data.months.map((m) => m.spending),
                  true,
                )}
              </tr>
              <tr className="net-row">
                <td>{t("Budget surplus")}</td>
                {cells(net, true)}
              </tr>
              <tr className="net-row">
                <td>{t("Budget surplus rate")}</td>
                {rates.map((r, i) => (
                  <td key={months[i]} className={`num${r !== null && r < 0 ? " negative" : ""}`}>
                    {r === null ? "—" : percent(r)}
                  </td>
                ))}
                <td className="num">
                  <strong>{data.income > 0 ? ltr(`${((kept / data.income) * 100).toFixed(1)}%`) : "—"}</strong>
                </td>
                <td className="num" />
                <td aria-hidden />
              </tr>
            </tbody>
          </table>
        </ReportTable>
      </Panel>
    </>
  );
}

/** Where income comes from: sources ranked, as a whole and month by month. */
export function IncomeBreakdown({ scope }: { scope: Scope }) {
  const { data } = scope;
  const n = Math.max(1, data.months.length);
  const all = Object.entries(data.incomeSources).map(([name, value]) => ({
    key: name,
    label: name || t("No payee"),
    value,
  }));
  const positive = all.filter((s) => s.value > 0).sort((a, b) => b.value - a.value);
  const negative = all.filter((s) => s.value < 0).sort((a, b) => a.value - b.value);
  const { coloured, folded } = ranked(positive, t("Other sources"));
  const total = sum(positive.map((s) => s.value));
  const topKeys = new Set(coloured.slice(0, 7).map((s) => s.key));
  const months = data.months.map((m) => m.month);
  const series = folded.map((s) => ({
    key: s.key,
    label: s.label,
    color: s.color,
    values: data.months.map((m) =>
      s.key === "__other"
        ? sum(
            Object.entries(m.incomeSources)
              .filter(([k, v]) => !topKeys.has(k) && v > 0)
              .map(([, v]) => v),
          )
        : Math.max(0, m.incomeSources[s.key] ?? 0),
    ),
  }));
  const withIncome = data.months.filter((m) => m.income > 0).length;
  const table = useReportColumns("income-sources", 4, 200);
  const top = positive[0];
  return (
    <>
      <Tiles
        items={[
          {
            label: t("Income"),
            value: fmt(data.income),
            note: negative.length
              ? t("after {amount} taken out", { amount: fmt(-sum(negative.map((s) => s.value))) })
              : tn(positive.length, "from {count} source", "from {count} sources"),
          },
          { label: t("Monthly average"), value: fmt(Math.round(data.income / n)) },
          {
            label: t("Largest source"),
            value: top?.label ?? "—",
            note: top ? `${fmt(top.value)} · ${share(top.value, total)}` : undefined,
          },
          { label: t("Months with income"), value: t("{count} of {total}", { count: withIncome, total: data.months.length }) },
        ]}
      />
      <Panel title={t("Where your income comes from")}>
        {positive.length ? (
          <div className="donut-layout">
            <Donut
              slices={folded}
              label={t("Income by source")}
              format={fmt}
              center={{ value: money(total), label: t("Positive sources") }}
            />
            <BarList
              rows={coloured.map((s) => ({ ...s, detail: share(s.value, total) }))}
              color={SERIES(0)}
              format={fmt}
              label={t("Income by source")}
            />
          </div>
        ) : (
          <p className="empty-state compact">{t("No income in these months.")}</p>
        )}
        {negative.length > 0 && (
          <p className="help">{t("The charts show positive source contributions. The income total and source table include amounts taken out of Unassigned.")}</p>
        )}
      </Panel>
      {positive.length > 0 && (
        <Panel title={t("Income by month")}>
          <ColumnChart months={months} series={series} label={t("Income each month, stacked by source")} format={fmt} />
          <Legend items={series.map((s) => ({ label: s.label, color: s.color }))} />
        </Panel>
      )}
      <Panel title={t("Sources")}>
        <ReportTable>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Source")}{table.grip("label", "Source")}</th>
                <th className="num">{t("Total")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Monthly average")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Share")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Months")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {[...positive, ...negative].map((s) => (
                <tr key={s.key}>
                  <td>
                    <button
                      className="link-button cell-link"
                      onClick={() => scope.show({ title: s.label, payee: s.key, income: true })}
                      title={t("Show these transactions")}
                    >
                      {s.label}
                    </button>
                  </td>
                  <td className={`num${s.value < 0 ? " negative" : ""}`}>{money(s.value)}</td>
                  <td className="num">{money(Math.round(s.value / n))}</td>
                  <td className="num">{s.value > 0 ? share(s.value, total) : "—"}</td>
                  <td className="num">{data.months.filter((m) => (m.incomeSources[s.key] ?? 0) !== 0).length}</td>
                  <td aria-hidden />
                </tr>
              ))}
            </tbody>
          </table>
        </ReportTable>
        {negative.length > 0 && (
          <p className="help">
            {t("Negative sources are amounts explicitly taken from Unassigned, such as a reconciliation adjustment or a transfer to a tracking account assigned to Unassigned.")}
          </p>
        )}
      </Panel>
    </>
  );
}

/**
 * Flows across the budget-account boundary, based on recorded account movements. Credit purchases
 * and tracking transfers count; transfers between budget accounts do not.
 */
export function CashFlow({ scope }: { scope: Scope }) {
  const { data } = scope;
  const months = data.months.map((m) => m.month);
  const n = Math.max(1, months.length);
  const inflow = data.months.map((m) => m.inflow ?? 0);
  const outflow = data.months.map((m) => m.outflow ?? 0);
  const net = inflow.map((v, i) => v - outflow[i]);
  let running = 0;
  const cumulative = net.map((v) => (running += v));
  const totalIn = sum(inflow),
    totalOut = sum(outflow);
  const table = useReportColumns("cash-flow", 4);
  return (
    <>
      <Tiles
        items={[
          { label: t("Money in"), value: fmt(totalIn), note: t("{amount} a month", { amount: fmt(Math.round(totalIn / n)) }) },
          { label: t("Money out"), value: fmt(totalOut), note: t("{amount} a month", { amount: fmt(Math.round(totalOut / n)) }) },
          { label: t("Net flow"), value: fmt(totalIn - totalOut), tone: totalIn - totalOut < 0 ? "negative" : "positive" },
          { label: t("Months in the black"), value: t("{count} of {total}", { count: net.filter((v) => v > 0).length, total: months.length }) },
        ]}
      />
      <Panel title={t("Money in and out by month")}>
        <ColumnChart
          months={months}
          series={[
            { key: "in", label: t("Money in"), color: SERIES(0), values: inflow },
            { key: "out", label: t("Money out"), color: SERIES(1), values: outflow.map((v) => -v) },
          ]}
          line={{ label: t("Net flow"), color: "var(--text)", values: net }}
          label={t("Money into budget accounts above the line, money out below, with the net as a line")}
          format={fmt}
        />
        <Legend
          items={[
            { label: t("Money in"), color: SERIES(0) },
            { label: t("Money out"), color: SERIES(1) },
            { label: t("Net flow"), color: "var(--text)", line: true },
          ]}
        />
        <p className="help">
          {t("Account movements across your budget boundary, including credit-card purchases and transfers to tracking accounts. Card payments, other transfers between budget accounts and starting balances are excluded. Split inflows and outflows may be netted; these are account movements rather than gross category splits.")}
        </p>
      </Panel>
      <Panel title={t("Running total")} aside={<span className="muted">{t("Net flow added up")}</span>}>
        <LinesChart
          keys={months}
          series={[{ key: "running", label: t("Running total"), color: SERIES(2), values: cumulative }]}
          label={t("Net flow added up month by month")}
          format={fmt}
          height={200}
        />
      </Panel>
      <Panel title={t("By month")}>
        <ReportTable>
          <table className="fixed" style={table.style}>
            {table.cols}
            <thead>
              <tr>
                <th>{t("Month")}{table.grip("label", "Month")}</th>
                <th className="num">{t("In")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Out")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Net")}{table.grip("value", "amount")}</th>
                <th className="num">{t("Running total")}{table.grip("value", "amount")}</th>
                <th aria-hidden />
              </tr>
            </thead>
            <tbody>
              {months.map((m, i) => (
                <tr key={m}>
                  <td>{monthName(m, "short")}</td>
                  <td className="num">{money(inflow[i])}</td>
                  <td className="num">{money(outflow[i])}</td>
                  <td className={`num${net[i] < 0 ? " negative" : ""}`}>{money(net[i])}</td>
                  <td className="num">{money(cumulative[i])}</td>
                  <td aria-hidden />
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <th>{t("Average")}</th>
                <th className="num">{money(Math.round(totalIn / n))}</th>
                <th className="num">{money(Math.round(totalOut / n))}</th>
                <th className="num">{money(Math.round((totalIn - totalOut) / n))}</th>
                <th className="num" />
                <th aria-hidden />
              </tr>
            </tfoot>
          </table>
        </ReportTable>
      </Panel>
    </>
  );
}
