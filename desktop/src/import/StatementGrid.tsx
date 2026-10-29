import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, LoaderCircle, MessageSquareText } from "lucide-react";
import type { BankFixes, BankSession, Statement, StatementLine } from "../bridge";
import type { Budget } from "../types";
import { Tabs } from "../ui";
import { useSteady } from "../useSteady";
import { LineTable, type LineOps } from "./LineTable";
import { fixesFor, readings, restructure, roles, shown, withRole, type Filter } from "./model";
import { t, tc, tn } from "../i18n";

/** A change to the import, made against it as it is when it's made. */
type Build = (session: BankSession, statement: Statement) => BankFixes;

/**
 * Every line of a bank file with what became of it, to fix: a cell that doesn't read, a line left
 * out to include, which column holds what, the header row, the account of several, the date order
 * the dates don't settle. Lines that still need attention stay out of the import.
 */
export function StatementGrid({
  budget,
  statement,
  session,
  busy,
  assisted,
  onFix,
  onContinue,
  onAssistant,
  checking = false,
}: {
  budget: Budget;
  statement: Statement;
  session: BankSession;
  /** A change is being applied, or the file read again: nothing else is changed meanwhile. */
  busy: boolean;
  /** Lines the assistant changed. */
  assisted: ReadonlySet<string>;
  onFix: (fixes: BankFixes) => void;
  onContinue: () => void;
  /** Hands the lines that need attention to the assistant. */
  onAssistant?: () => void;
  /** On to the review: its lines are being checked against the budget. */
  checking?: boolean;
}) {
  // Most changes take a moment: the grid only looks busy (a progress line, its lines faded) for
  // one that lasts, so a quick one doesn't flash it.
  const slow = useSteady(busy, (b) => b, 300);
  const [filter, setFilter] = useState<Filter>(statement.counts.attention ? "attention" : "all");
  const lines = useMemo(() => statement.lines.filter((l) => shown(l, filter)), [statement.lines, filter]);
  // Each tab's count, once per reading of the file rather than on every scroll.
  const counts = useMemo(() => {
    let leftOut = 0;
    let edited = 0;
    for (const l of statement.lines) {
      if (l.status.kind === "leftOut") leftOut++;
      if (l.fixed || l.included) edited++;
    }
    return { all: statement.lines.length, attention: statement.counts.attention, leftOut, edited };
  }, [statement]);
  const table = statement.format === "csv";
  const role = useMemo(() => (table ? roles(statement.headers, statement.columns) : ["date", "payee", "memo", "amount"] as const), [table, statement.headers, statement.columns]);
  const s = session.structure;

  // Changes read the import as it is now. Those asked for while another applies wait for it, in
  // turn, each made against the import as the one before it left it.
  const latest = useRef({ session, statement, onFix, busy });
  latest.current = { session, statement, onFix, busy };
  const queued = useRef<Build[]>([]);
  const ops = useMemo<LineOps & { send: (build: Build) => void; next: () => void }>(() => {
    const next = () => {
      const now = latest.current;
      const build = now.busy ? undefined : queued.current.shift();
      if (build) now.onFix(build(now.session, now.statement));
    };
    const send = (build: Build) => {
      queued.current.push(build);
      next();
    };
    // Lines' changes, as one, against the lines as they read when it's made.
    const onLines = (lines: StatementLine[], change: (ls: StatementLine[]) => Parameters<typeof fixesFor>[1]) =>
      send((now, st) => {
        const by = new Map(st.lines.map((l) => [l.id, l]));
        return fixesFor(now, change(lines.map((line) => by.get(line.id) ?? line)));
      });
    return {
      send,
      next,
      // Each from the value it was changed from, so the engine turns away one changed meanwhile.
      cells: (line, changes, fields = []) =>
        onLines([line], ([l]) => ({
          cells: changes.map(({ column, from, to }) => ({ line: l.id, lineVersion: l.version, column, from, to })),
          ...(fields.length ? { fields: fields.map((f) => ({ ...f, line: l.id, lineVersion: l.version })) } : {}),
        })),
      include: (lines, on) => onLines(lines, (ls) => ({ include: ls.map((l) => ({ line: l.id, lineVersion: l.version, include: on })) })),
      revert: (lines) => onLines(lines, (ls) => ({ revert: ls.map((l) => ({ line: l.id, lineVersion: l.version })) })),
      header: (line) => send((now) => restructure(now, { headerLine: Number(line.id) })),
      whole: (entry, on) =>
        send((now) => {
          const was = now.structure.wholeBatches ?? [];
          return restructure(now, { wholeBatches: on ? [...was, entry] : was.filter((e) => e !== entry) });
        }),
      role: (column, r) => send((now, st) => restructure(now, { columns: withRole(st.headers, st.columns, column, r) })),
    };
  }, []);
  // The next one waiting, once the one before it has applied and the file reads again.
  useEffect(() => {
    if (!busy) ops.next();
  }, [busy]);

  const act = ops.send;
  const question = statement.dateOrder?.unconfirmed ? statement.dateOrder : null;
  const answered = statement.dateOrder && !statement.dateOrder.unconfirmed ? statement.dateOrder : null;
  const named = (question ?? answered) && readings((question ?? answered)!.example);
  // A file in another currency than the budget's: its lines are left out, each saying which.
  const foreign = useMemo(
    () => statement.lines.find((l) => l.status.kind === "leftOut" && l.status.reason === "otherCurrency")?.status,
    [statement.lines],
  );
  const elsewhere = !statement.counts.ready && foreign?.kind === "leftOut" ? foreign.detail : null;
  const attention = statement.counts.attention;
  const sources = statement.sources ?? [];
  const order = (o: "dmy" | "mdy") => act((now) => restructure(now, { dateOrder: { order: o, confirmed: true } }));
  return (
    <div className={`statement-editor${slow ? " slow" : ""}`}>
      <div className="statement-bar">
        {sources.length > 1 && (
          <label className="statement-source">
            {t("Account in the file")}
            <select
              value={statement.source ?? 0}
              disabled={slow}
              onChange={(e) => {
                const source = sources[Number(e.target.value)].label;
                act((now) => restructure(now, { source }));
              }}
            >
              {sources.map((source, i) => (
                <option key={source.label} value={i}>
                  {source.label}
                  {source.currency ? ` · ${source.currency}` : ""} · {tn(source.lines, "{count} line", "{count} lines")}
                </option>
              ))}
            </select>
          </label>
        )}
        {answered && (
          // Answered once, the date order stays one choice away.
          <label className="statement-choice">
            {t("Dates")}
            <select value={answered.order} disabled={slow} onChange={(e) => order(e.target.value as "dmy" | "mdy")}>
              <option value="dmy">
                {named ? t("Day first ({example} is {date})", { example: answered.example, date: named.dmy }) : t("Day first")}
              </option>
              <option value="mdy">
                {named ? t("Month first ({example} is {date})", { example: answered.example, date: named.mdy }) : t("Month first")}
              </option>
            </select>
          </label>
        )}
        {statement.signs && !statement.signs.unconfirmed && (
          <label className="statement-choice">
            {t("Money out")}
            <select
              value={statement.signs.byType ? "type" : "written"}
              disabled={slow}
              onChange={(e) => {
                const signsByType = e.target.value === "type";
                act((now) => restructure(now, { signsByType }));
              }}
            >
              <option value="type">{t("Told by the transaction types")}</option>
              <option value="written">{t("As the amounts are written")}</option>
            </select>
          </label>
        )}
        {table && (
          // Some card issuers write a purchase as a positive number and a payment as a negative one.
          <label
            className="statement-choice"
            title={t("How the file writes a purchase. Card statements often write purchases as positive numbers and payments as negative ones; choosing that turns every amount's sign.")}
          >
            {t("Purchases in the file")}
            <select
              value={s.flipSigns ? "positive" : "negative"}
              disabled={slow}
              onChange={(e) => {
                // Read as chosen now: the change may wait for one before it.
                const flipSigns = e.target.value === "positive";
                act((now) => restructure(now, { flipSigns }));
              }}
            >
              <option value="negative">{t("Negative, as bank accounts write them")}</option>
              <option value="positive">{t("Positive, as many card statements write them")}</option>
            </select>
          </label>
        )}
      </div>
      {elsewhere && !statement.currency && (
        <p className="statement-question" role="status">
          <AlertTriangle size={15} aria-hidden />
          <span>
            {t("The file's amounts are in {currency}. Include a line to enter its amount in your budget's currency, or import the file into a budget in {currency}.", {
              currency: elsewhere,
            })}
          </span>
        </p>
      )}
      {statement.currency && (
        <p className="statement-question" role="status">
          <AlertTriangle size={15} aria-hidden />
          <span>
            {t("This account's amounts are in {currency}. Choose another account in the file, or include its lines with their amounts in your budget's currency.", {
              currency: statement.currency,
            })}
          </span>
        </p>
      )}
      {question && (
        <div className="statement-question" role="group" aria-label={t("Date order")}>
          <AlertTriangle size={15} aria-hidden />
          <span>
            {t("The dates don't say whether the day or the month comes first: is {example}", { example: <b>{question.example}</b> })}
          </span>
          <span className="statement-answers">
            {(["dmy", "mdy"] as const).map((o) => (
              <button
                key={o}
                className={question.order === o ? "primary small" : "secondary small"}
                disabled={slow}
                onClick={() => order(o)}
              >
                {named ? named[o] : o === "dmy" ? t("Day first") : t("Month first")}
              </button>
            ))}
          </span>
        </div>
      )}
      {statement.signs?.unconfirmed && (
        <div className="statement-question" role="group" aria-label={t("Signs")}>
          <AlertTriangle size={15} aria-hidden />
          <span>{t("Every amount that reads is positive. Is money out told by the transaction types (DEBIT, POS…)?")}</span>
          <span className="statement-answers">
            <button className="primary small" disabled={slow} onClick={() => act((now) => restructure(now, { signsByType: true }))}>
              {t("Yes, use the types")}
            </button>
            <button className="secondary small" disabled={slow} onClick={() => act((now) => restructure(now, { signsByType: false }))}>
              {t("No, as written")}
            </button>
          </span>
        </div>
      )}
      <Tabs
        label={t("Show lines")}
        value={filter}
        onChange={setFilter}
        tabs={[
          { id: "all", label: tc("lines", "All"), count: counts.all },
          { id: "attention", label: t("Need attention"), count: counts.attention },
          { id: "leftOut", label: t("Left out"), count: counts.leftOut },
          { id: "edited", label: t("Edited"), count: counts.edited },
        ]}
      />
      <div className="statement-grid-frame">
        {/* A change that takes a while shows as a line moving along the top, not a dimmed grid. */}
        <div className="statement-progress" aria-hidden />
        <LineTable
          // Another layout (another header row) is another table, with its own column widths.
          key={statement.headers.join("\u0001")}
          budget={budget}
          statement={statement}
          lines={lines}
          role={role}
          busy={busy}
          slow={slow}
          assisted={assisted}
          ops={ops}
          empty={
            <p className="help">
              {filter === "attention" ? t("Every line reads now.") : filter === "edited" ? t("Nothing changed yet.") : t("No lines here.")}{" "}
              <button className="link-button" onClick={() => setFilter("all")}>
                {t("Show all lines")}
              </button>
            </p>
          }
        />
      </div>
      <div className="form-actions statement-actions">
        <span className="help" role="status">
          {checking
            ? t("Checking the lines against your budget…")
            : question
              ? t("Say whether the day or the month comes first, above, to continue.")
              : statement.signs?.unconfirmed
                ? t("Say how the file tells money out, above, to continue.")
                : !statement.counts.ready
                  ? t("No line reads yet.")
                  : `${tn(statement.counts.ready, "{count} line reads", "{count} lines read")}${
                      attention > 0
                        ? ` · ${tn(attention, "{count} needs attention and won't be imported", "{count} need attention and won't be imported")}`
                        : ""
                    }`}
        </span>
        {onAssistant && attention > 0 && (
          <button className="secondary" disabled={slow} onClick={onAssistant}>
            <MessageSquareText size={14} /> {t("Fix with the assistant")}
          </button>
        )}
        <button
          className="primary"
          // Nothing goes on to the review on a guess the file leaves open.
          disabled={busy || checking || !statement.counts.ready || !!question || !!statement.signs?.unconfirmed}
          onClick={onContinue}
        >
          {checking && <LoaderCircle size={14} className="spin" aria-hidden />}
          {t("Continue to the review")}
        </button>
      </div>
    </div>
  );
}
