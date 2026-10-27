import { useEffect, useMemo, useState } from "react";
import { api } from "./bridge";
import { Modal } from "./forms";
import { DateInput } from "./ui";
import { Legend, LinesChart } from "./charts/Charts";
import { accountType } from "./accounts";
import { monthTick } from "./charts/scale";
import { editableAmount, errorText, evaluateAmount, formatDate, money, monthName } from "./format";
import { nextPayment, owedOn, percent, rateIn, useProjection } from "./Planning";
import { today, type Account, type Budget, type LoanProjection, type LoanTerms } from "./types";
import { t, tn } from "./i18n";
import "./reports.css";
import "./loans.css";

/** "2 years 3 months" */
export function span(months: number) {
  const years = Math.floor(months / 12);
  const rest = months % 12;
  const y = years ? tn(years, "{count} year", "{count} years") : "";
  const m = rest ? tn(rest, "{count} month", "{count} months") : "";
  if (y && m) return t("{years} {months}", { years: y, months: m });
  return y || m || tn(0, "{count} month", "{count} months");
}
/** A typed amount, or null when it doesn't read; blank is 0. */
const amountOf = (text: string) => {
  try {
    return Math.max(0, evaluateAmount(text));
  } catch {
    return null;
  }
};
const CURRENT = "var(--muted)";
const CHANGED = "var(--accent)";

/** The balance after each payment, current plan against the changes, from today's balance. */
function PayoffChart({ owed, start, base, scenario }: { owed: number; start: number; base: LoanProjection | null; scenario: LoanProjection | null }) {
  const longest = [base, scenario].reduce<LoanProjection | null>(
    (a, p) => (p && (!a || p.schedule.length > a.schedule.length) ? p : a),
    null,
  );
  if (!longest) return null;
  // The first point is now; then one per payment, keyed by its date.
  const keys = ["now", ...longest.schedule.map((r) => r.date)];
  const values = (p: LoanProjection | null, from: number) =>
    p ? keys.map((_, i) => (i === 0 ? from : (p.schedule[i - 1]?.balance ?? null))) : [];
  const many = keys.length > 25;
  return (
    <div className="loan-chart">
      <LinesChart
        keys={keys}
        series={[
          ...(base ? [{ key: "current", label: t("Current plan"), color: CURRENT, values: values(base, owed), dashed: !!scenario }] : []),
          ...(scenario ? [{ key: "changed", label: t("With changes"), color: CHANGED, values: values(scenario, start) }] : []),
        ]}
        label={t("Balance owed after each payment")}
        format={money}
        height={200}
        title={(k) => (k === "now" ? t("Today") : formatDate(k))}
        // Every month's name when there are few, else each new year.
        tick={(k, i) =>
          k === "now"
            ? t("Now")
            : many
              ? k.slice(5, 7) === "01"
                ? k.slice(0, 4)
                : null
              : monthTick(k.slice(0, 7), i > 1 ? keys[i - 1].slice(0, 7) : undefined)
        }
      />
      {base && scenario && (
        <Legend
          items={[
            { label: t("Current plan"), color: CURRENT, line: true, dashed: true },
            { label: t("With changes"), color: CHANGED, line: true },
          ]}
        />
      )}
    </div>
  );
}

function Schedule({ projection, fees }: { projection: LoanProjection; fees: string }) {
  return (
    <details className="loan-schedule">
      <summary>
        {tn(projection.schedule.length, "Payment schedule ({count} payments)", "Payment schedule ({count} payments)")}
      </summary>
      <div className="loan-schedule-scroll">
        <table className="data-table">
          <thead>
            <tr>
              <th>{t("Date")}</th>
              <th className="num">{t("Payment")}</th>
              <th className="num">{t("Interest")}</th>
              <th className="num">{fees}</th>
              <th className="num">{t("Principal")}</th>
              <th className="num">{t("Still owed")}</th>
            </tr>
          </thead>
          <tbody>
            {projection.schedule.map((r) => (
              <tr key={r.date}>
                <td>{formatDate(r.date)}</td>
                <td className="num">{money(r.payment)}</td>
                <td className="num">{money(r.interest)}</td>
                <td className="num">{money(r.fees)}</td>
                <td className="num">{money(r.principal)}</td>
                <td className="num">{money(r.balance)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  );
}

/**
 * Trying out ways to pay a loan off sooner: more each month, a one-off payment now, or the
 * payment that clears it by a date. A sandbox: it never changes the budget or the loan's details.
 */
export function LoanPlanner({
  budget,
  account,
  terms,
  balance,
  close,
}: {
  budget: Budget;
  account: Account;
  terms: LoanTerms;
  /** The account's working balance. */
  balance: number;
  close: () => void;
}) {
  const owed = owedOn(balance);
  const start = useMemo(() => nextPayment(budget, account.id), [budget.transactions, account.id]);
  const [extraText, setExtraText] = useState("");
  const [oneOffText, setOneOffText] = useState("");
  const [by, setBy] = useState("");
  const extra = amountOf(extraText);
  const oneOff = amountOf(oneOffText);
  // Recording a payment adds the month's interest and fees with the month's first payment. While
  // this month's is still to come, a one-off now goes in with it, after them (Rust's
  // project_with); once it's in, they're charged and the one-off comes off what's owed.
  const firstThisMonth = start.slice(0, 7) === today().slice(0, 7);
  const lump = firstThisMonth ? (oneOff ?? 0) : 0;
  const left = firstThisMonth ? owed : Math.max(0, owed - (oneOff ?? 0));
  const base = useProjection(owed, terms, start);
  const changed = !!extra || !!oneOff;
  const scenario = useProjection(changed ? left : 0, terms, start, extra ?? 0, lump);
  // The payment that clears what's left by the chosen date; the difference becomes the extra.
  const neededKey = JSON.stringify([by, left, lump, start, terms]);
  const [neededResult, setNeeded] = useState<{ key: string; amount: number; error: string } | null>(null);
  const needed = by && left > 0 && neededResult?.key === neededKey ? neededResult : null;
  useEffect(() => {
    if (!by || left <= 0) return;
    let live = true;
    Promise.resolve()
      .then(() => api.loanPaymentByDate(left, terms, start, by, lump))
      .then(
        (amount) => {
          if (!live) return;
          setNeeded({ key: neededKey, amount, error: "" });
          const more = amount - terms.monthlyPayment;
          setExtraText(more > 0 ? editableAmount(more) : "");
        },
        (e) => live && setNeeded({ key: neededKey, amount: 0, error: errorText(e) }),
      );
    return () => {
      live = false;
    };
  }, [neededKey]);
  const now = base.projection;
  const then = changed && left > 0 ? scenario.projection : null;
  // Paid off now: the one-off alone clears the loan, with any interest and fees it brings.
  const paidNow = changed && (then ? then.months === 1 && (oneOff ?? 0) >= then.schedule[0].payment : left === 0);
  const fees = accountType(account, balance) === "mortgage" ? t("Escrow") : t("Fees");
  // Each plan's figures; the total includes what's owed now (a one-off payment is part of it).
  const rows = [
    { label: t("Paid off"), value: (p: LoanProjection) => monthName(p.payoffDate.slice(0, 7)) },
    { label: t("Time left"), value: (p: LoanProjection) => span(p.months) },
    { label: t("Interest"), value: (p: LoanProjection) => money(p.totalInterest) },
    ...(terms.monthlyFees ? [{ label: fees, value: (p: LoanProjection) => money(p.totalFees) }] : []),
    { label: t("Total cost"), value: (p: LoanProjection) => money(owed + p.totalInterest + p.totalFees) },
  ];
  return (
    <Modal title={t("Payoff planner · {account}", { account: account.name })} close={close} wide className="loan-planner">
      <dl className="loan-facts">
        <div>
          <dt>{t("Owed")}</dt>
          <dd>{money(owed)}</dd>
        </div>
        <div>
          <dt>{t("Interest rate")}</dt>
          <dd>{percent(rateIn(terms, today().slice(0, 7)))}</dd>
        </div>
        <div>
          <dt>{t("Monthly payment")}</dt>
          <dd>{money(terms.monthlyPayment)}</dd>
        </div>
      </dl>
      <div className="form-grid loan-try">
        <label>
          {t("Extra each month")}
          <input
            aria-label={t("Extra each month")}
            inputMode="decimal"
            placeholder="0.00"
            value={extraText}
            onChange={(e) => setExtraText(e.target.value)}
          />
        </label>
        <label>
          {t("One-off payment now")}
          <input
            aria-label={t("One-off payment")}
            inputMode="decimal"
            placeholder="0.00"
            value={oneOffText}
            onChange={(e) => setOneOffText(e.target.value)}
          />
        </label>
        <label>
          {t("Pay off by")}
          <DateInput label={t("Pay off by")} value={by} onChange={setBy} />
        </label>
      </div>
      {(extra === null || oneOff === null) && (
        <p className="form-error" role="alert">
          {t("Enter amounts like 50 or 25.50.")}
        </p>
      )}
      {needed && by && (
        <p className="loan-needed" role="status">
          {needed.error
            ? needed.error
            : needed.amount > terms.monthlyPayment
              ? t("To pay it off by {month}, pay {amount} a month: {more} more than now, tried below as the extra.", {
                  month: monthName(by.slice(0, 7)),
                  amount: money(needed.amount, { symbol: true }),
                  more: money(needed.amount - terms.monthlyPayment),
                })
              : t("Your current payment already pays it off by {month}.", { month: monthName(by.slice(0, 7)) })}
        </p>
      )}
      {base.error && (
        <p className="form-error" role="alert">
          {base.error}
        </p>
      )}
      {changed && scenario.error && (
        <p className="form-error" role="alert">
          {scenario.error}
        </p>
      )}
      {now && (then || paidNow) && (
        <div className="loan-savings">
          <div>
            <span>{t("Interest saved")}</span>
            <strong>{money(now.totalInterest - (then?.totalInterest ?? 0))}</strong>
          </div>
          <div>
            <span>{t("Time saved")}</span>
            <strong>{span(now.months - (then?.months ?? 0))}</strong>
          </div>
          <div>
            <span>{t("Paid off")}</span>
            <strong>{paidNow || !then ? t("Now") : monthName(then.payoffDate.slice(0, 7))}</strong>
          </div>
        </div>
      )}
      {now && (
        <table className="data-table loan-compare">
          <thead>
            <tr>
              <th />
              <th className="num">{t("Current plan")}</th>
              {changed && <th className="num">{t("With changes")}</th>}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.label}>
                <th scope="row">{r.label}</th>
                <td className="num">{r.value(now)}</td>
                {changed && <td className="num">{then ? r.value(then) : paidNow ? "—" : ""}</td>}
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <PayoffChart owed={owed} start={left} base={now} scenario={then} />
      {(then ?? now) && <Schedule projection={(then ?? now)!} fees={fees} />}
      <p className="help">
        {t("Only a sandbox: nothing here changes your budget or the loan's details. Interest compounds monthly and each charge is rounded to the cent, so a lender's own figures may differ a little.")}
      </p>
    </Modal>
  );
}
