import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { money } from "../format";
import { ltr, t } from "../i18n";
import { tableWidth, textWidth, useColumns } from "../ui/columns";
import type { Budget } from "../types";
import type { RegisterFilter } from "../Register";

export type Month = {
  month: string;
  income: number;
  spending: number;
  netIncome: number;
  assets: number;
  debt: number;
  netWorth: number;
  /** Money into budget accounts from outside them. */
  inflow?: number;
  /** Money out of budget accounts, as a positive amount. */
  outflow?: number;
  categories: Record<string, number>;
  incomeSources: Record<string, number>;
  accounts: Record<string, number>;
};
export type ReportData = {
  months: Month[];
  categories: Record<string, number>;
  incomeSources: Record<string, number>;
  /** Spending by payee, and how many transactions it took. */
  payees?: Record<string, { amount: number; count: number }>;
  /** Unique saved transactions contributing spending, including refunds, across all payees. */
  transactionCount?: number;
  /** Spending on each day that had any. */
  days?: Record<string, number>;
  income: number;
  spending: number;
  ageOfMoney: number | null;
  ageOfMoneyByMonth: Record<string, number | null>;
  /** Balances just before the range starts. */
  openingAccounts?: Record<string, number>;
};

/** Everything a report needs: the data, the budget, the filters and a way into the register. */
export type Scope = {
  data: ReportData;
  budget: Budget;
  /** The range, as months. */
  from: string;
  to: string;
  /** Chosen accounts; empty means all. */
  accounts: Set<string>;
  /** Whether only some categories are chosen. */
  someCategories: boolean;
  groupOf: Map<string, string>;
  nameOf: Map<string, string>;
  /** Groups that hold spending, in budget order. */
  spendingGroups: string[];
  /** A group's colour: the first seven groups in budget order have their own. */
  colorOf: (group: string) => string;
  /** Opens the register on these transactions (dates default to the range). */
  inspect: (filter: RegisterFilter) => void;
  /** Lists the transactions behind a figure in a dialog (dates default to the range). */
  show: (figure: Figure) => void;
};
/** A figure whose transactions can be listed: a payee, a category, an income source or a day. */
export type Figure = { title: string; payee?: string; categoryId?: string; income?: boolean; from?: string; to?: string };

export const SERIES = (i: number) => `var(--series-${i + 1})`;
export const OTHER = "var(--series-other)";
export const fmt = (n: number) => money(n, { symbol: true });
export const sum = (values: number[]) => values.reduce((n, v) => n + v, 0);
export const share = (n: number, total: number) => (total > 0 ? ltr(`${((n / total) * 100).toFixed(1)}%`) : "—");
// The sign stays with the amount in Arabic too.
export const signed = (n: number) => ltr(`${n > 0 ? "+" : ""}${money(n)}`);
export const firstDay = (month: string) => `${month}-01`;
export const lastDay = (month: string) => {
  const [y, m] = month.split("-").map(Number);
  return `${month}-${String(new Date(y, m, 0).getDate()).padStart(2, "0")}`;
};

/**
 * Colours for a ranked list: the first seven items have their own colour; the rest share
 * "Other" and are folded into one item for charts.
 */
export function ranked<T extends { key: string; label: string; value: number }>(items: T[], otherLabel = t("Other")) {
  const coloured = items.map((item, i) => ({ ...item, color: i < 7 ? SERIES(i) : OTHER }));
  const rest = coloured.slice(7);
  const folded = rest.length
    ? [
        ...coloured.slice(0, 7),
        { key: "__other", label: otherLabel, value: sum(rest.map((r) => r.value)), color: OTHER },
      ]
    : coloured;
  return { coloured, folded };
}

/**
 * Holds a report table. It grows with the page, and its header sticks to the top of the report
 * as you scroll; only a table too wide for its panel scrolls in a box of its own, about a window
 * tall. `wide` keeps the first column in view while scrolling sideways.
 */
export function ReportTable({ wide = false, children }: { wide?: boolean; children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const [scrolls, setScrolls] = useState(false);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const check = () => setScrolls(el.scrollWidth > el.clientWidth + 1);
    check();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(check);
    observer.observe(el);
    if (el.firstElementChild) observer.observe(el.firstElementChild);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={box} className={`report-table${wide ? " wide" : ""}${scrolls ? " scrolls" : ""}`}>
      {children}
    </div>
  );
}

/**
 * A heading row inside a report table. The label sits in the first column, so it stays in view
 * with the names when a wide table scrolls sideways.
 */
export function SectionRow({ title, columns }: { title: string; columns: number }) {
  return (
    <tr className="section-row">
      <th scope="rowgroup">{title}</th>
      <td colSpan={columns - 1} />
    </tr>
  );
}

/** Fixed widths for a report table: a label column, value columns sharing one width, and spare room. */
export function useReportColumns(id: string, values: number, label = 170) {
  const group = useRef<HTMLTableColElement>(null);
  const [contentWidth, setContentWidth] = useState(112);
  useLayoutEffect(() => {
    const table = group.current?.parentElement;
    if (!table) return;
    // Scan strings, not cell geometry. Body and footer may use different font sizes, so
    // measure their longest values separately using the actual responsive font and padding.
    let next = 112;
    for (const section of ["tbody", "tfoot"]) {
      let longest = "";
      let widest: Element | undefined;
      for (const cell of table.querySelectorAll(`${section} td.num, ${section} th.num`)) {
        const text = cell.textContent?.trim() ?? "";
        if (text.length > longest.length) {
          longest = text;
          widest = cell;
        }
      }
      if (!widest) continue;
      const style = getComputedStyle(widest);
      const pixels = (value: string) => Number.parseFloat(value) || 0;
      const size = pixels(style.fontSize) || 13;
      // A subtotal with the same number of digits can be heavier than the chosen body cell.
      const weight = Math.max(650, pixels(style.fontWeight));
      const padding = pixels(style.paddingLeft) + pixels(style.paddingRight);
      const borders = pixels(style.borderLeftWidth) + pixels(style.borderRightWidth);
      const spacing = Math.max(0, pixels(style.letterSpacing)) * longest.length;
      next = Math.max(next, Math.ceil(textWidth(longest, size, weight, true) + padding + borders + spacing + 2));
    }
    if (next !== contentWidth) setContentWidth(next);
  });
  const columns = useColumns(`report-${id}`, { label, value: contentWidth }, { value: contentWidth });
  const widths = [columns.widths.label, ...Array<number>(values).fill(columns.widths.value), null];
  return {
    grip: columns.grip,
    cols: <colgroup ref={group}>{widths.map((width, index) => <col key={index} style={width === null ? undefined : { width }} />)}</colgroup>,
    style: { minWidth: tableWidth(widths) },
  };
}

export type Tile = { label: string; value: ReactNode; note?: ReactNode; tone?: "negative" | "positive" };
/** The headline figures of a report. */
export function Tiles({ items }: { items: Tile[] }) {
  return (
    <dl className="report-tiles">
      {items.map((t) => (
        <div key={t.label}>
          <dt>{t.label}</dt>
          <dd className={t.tone ?? ""}>{t.value}</dd>
          {t.note && <dd className="tile-note">{t.note}</dd>}
        </div>
      ))}
    </dl>
  );
}

/** A titled panel inside a report. */
export function Panel({
  title,
  aside,
  children,
  className = "",
}: {
  title: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel report-panel ${className}`}>
      <div className="panel-title">
        <h2>{title}</h2>
        {aside && <span className="num">{aside}</span>}
      </div>
      {children}
    </section>
  );
}
