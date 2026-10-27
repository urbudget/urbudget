import { money, monthName } from "./format";
import type { Budget, YnabScreenDifference } from "./types";
import { list, msg, t, tn } from "./i18n";

// Enough to see a pattern without filling the panel.
const LIMIT = 20;
const amount = (n: number | null) => (n === null ? t("none") : money(n));
const days = (n: number | null) => (n === null ? t("none") : tn(n, "{count} day", "{count} days"));

/**
 * How an imported budget compares with what YNAB's own screens showed: Ready to Assign with
 * later months' assignments held back, age of money, and what each category is short. Unlike
 * the API checks, these can differ without stopping an import. Imports made before these
 * checks existed show nothing.
 */
export function YnabScreenAudit({ audit }: { audit: NonNullable<Budget["importAudit"]> }) {
  const { shownReadyToAssignDifferences: ready, ageOfMoneyDifferences: age, underfundedDifferences: under } = audit;
  if (!ready || !age || !under) return null;
  const kinds = [
    {
      label: msg("Ready to Assign"),
      what: msg("Ready to Assign"),
      count: (n: number) => tn(n, "Ready to Assign in {count} month", "Ready to Assign in {count} months"),
      rows: ready,
      show: amount,
    },
    {
      label: msg("Age of money"),
      what: msg("age of money"),
      count: (n: number) => tn(n, "age of money in {count} month", "age of money in {count} months"),
      rows: age,
      show: days,
    },
    {
      label: msg("Underfunded"),
      what: msg("underfunded amounts"),
      count: (n: number) =>
        tn(n, "underfunded amounts in {count} category month", "underfunded amounts in {count} category months"),
      rows: under,
      show: amount,
    },
  ].filter((k) => k.rows.length > 0);
  if (kinds.length === 0)
    return <p>{t("Ready to Assign, age of money and underfunded amounts match what YNAB's screens show in every month.")}</p>;
  const counts = kinds.map((k) => k.count(k.rows.length));
  return (
    <>
      <p>
        {t("Compared with what YNAB's screens show, these differ: {differences}.", {
          differences:
            counts.length > 1
              ? t("{items} and {last}", { items: list(counts.slice(0, -1)), last: counts[counts.length - 1] })
              : counts[0],
        })}
      </p>
      <details>
        <summary>{t("Show differences")}</summary>
        <ul>
          {kinds.flatMap((k) => {
            const rows = [...k.rows].sort((a, b) => a.month.localeCompare(b.month));
            const items = rows.slice(0, LIMIT).map((d: YnabScreenDifference, i) => {
              const amounts = t("{actual} here, {expected} in YNAB", { actual: k.show(d.actual), expected: k.show(d.expected) });
              return (
                <li key={`${k.label}-${i}`}>
                  {d.category
                    ? t("{label}, {category}, {month}: {amounts}", {
                        label: t(k.label),
                        category: d.category,
                        month: monthName(d.month),
                        amounts,
                      })
                    : t("{label}, {month}: {amounts}", { label: t(k.label), month: monthName(d.month), amounts })}
                </li>
              );
            });
            if (rows.length > LIMIT)
              items.push(
                <li key={`${k.label}-more`}>
                  {tn(rows.length - LIMIT, "…and {count} more for {what}", "…and {count} more for {what}", { what: t(k.what) })}
                </li>,
              );
            return items;
          })}
        </ul>
      </details>
    </>
  );
}
