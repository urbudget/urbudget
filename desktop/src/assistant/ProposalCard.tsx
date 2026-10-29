import { useEditorCheckpoint } from "../editorCheckpoint";
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { Check, Eye, FileDown, FileSpreadsheet, LoaderCircle, Pencil, RefreshCw, TriangleAlert, Undo2 } from "lucide-react";
import { editableAmount, errorText, evaluateAmount, money } from "../format";
import type { Budget } from "../types";
import { CategoryPicker, READY_TO_ASSIGN } from "../ui";
import type { CardRow, Proposal, Revision } from "./api";
import { ltr, msg, t, tn } from "../i18n";

// The sign stays with the amount in Arabic too.
const signed = (n: number) => (n > 0 ? ltr(`+${money(n)}`) : money(n));
/** Rows a card lists before "Show all". */
const SHOWN = 8;

type RowChange = Revision["set"][number];
const NONE: Revision = { skip: [], set: [] };

function Row({
  row,
  budget,
  choosing,
  busy,
  onToggle,
  onEdit,
}: {
  budgetId?: string;
  row: CardRow;
  budget: boolean;
  /** Its checkbox and editing are offered: the card is waiting to be applied. */
  choosing?: boolean;
  busy?: boolean;
  onToggle?: () => void;
  onEdit?: () => void;
}) {
  const editable = choosing && !row.skipped && (row.edit?.category || row.edit?.amount);
  return (
    <div className={`proposal-row${row.tone === "warn" ? " warn" : ""}${row.skipped ? " skipped" : ""}${row.edited ? " edited" : ""}`}>
      {choosing && row.key && (
        <input
          type="checkbox"
          className="proposal-pick"
          checked={!row.skipped}
          disabled={busy}
          aria-label={t("Apply: {item}", { item: row.label })}
          onChange={onToggle}
        />
      )}
      <span className="proposal-label">
        <span title={row.label}>{row.label}</span>
        {row.detail && <small title={row.detail}>{row.detail}</small>}
      </span>
      {row.badge && <span className={`proposal-badge${row.tone === "warn" ? " warn" : ""}`}>{row.badge}</span>}
      {budget && row.amount !== undefined ? (
        <span className={`num${row.amount > 0 ? " positive" : ""}`} title={row.edited ? t("Changed by you") : undefined}>
          {signed(row.amount)}
        </span>
      ) : (
        row.amount !== undefined && (
          <span className="num" title={row.edited ? t("Changed by you") : undefined}>
            {money(row.amount)}
          </span>
        )
      )}
      {row.before !== undefined && row.after !== undefined && row.before !== row.after && (
        <span className="num muted proposal-change">
          {ltr(`${money(row.before)} → ${money(row.after)}`)}
        </span>
      )}
      {editable && (
        <button
          type="button"
          className="proposal-edit"
          disabled={busy}
          aria-label={t("Change {item}", { item: row.label })}
          title={
            row.edit?.category && row.edit.amount
              ? t("Change the category or amount")
              : row.edit?.category
                ? t("Change the category")
                : t("Change the amount")
          }
          onClick={onEdit}
        >
          <Pencil size={12} aria-hidden />
        </button>
      )}
    </div>
  );
}

/**
 * A row's category and amount, as the register's editor has them: Enter (in the amount) or Save
 * keeps them, Esc leaves them as they were. An amount typed without a sign keeps the row's.
 */
function RowEditor({
  budgetId = "",
  row,
  budget,
  busy,
  onSave,
  onCancel,
}: {
  budgetId?: string;
  row: CardRow;
  budget: Pick<Budget, "categories" | "categoryOrder" | "groupOrder">;
  /** The card is being made again or applied: what's saved now would be made on the card as it was. */
  busy?: boolean;
  onSave: (change: Omit<RowChange, "key">) => Promise<void> | undefined;
  onCancel: () => void;
}) {
  const was = row.edit?.categoryId ?? READY_TO_ASSIGN;
  const [category, setCategory] = useState(was);
  const [amount, setAmount] = useState(row.amount !== undefined ? editableAmount(row.amount) : "");
  const [problem, setProblem] = useState("");
  const draft = useEditorCheckpoint(budgetId, "assistantRow", row, e => setProblem(errorText(e)), {category,amount}, ".proposal-editor input");
  const cancel = () => { draft.discard(); onCancel(); };
  const save = () => {
    if (busy) return;
    const change: Omit<RowChange, "key"> = {};
    if (row.edit?.category && category !== was) change.category = category === READY_TO_ASSIGN ? null : category;
    if (row.edit?.amount) {
      try {
        let cents = evaluateAmount(amount);
        if (!/^\s*[-+]/.test(amount) && (row.amount ?? 0) < 0) cents = -Math.abs(cents);
        if (cents !== row.amount) change.amount = cents;
      } catch (e) {
        setProblem(errorText(e));
        return;
      }
    }
    if (change.category === undefined && change.amount === undefined) cancel();
    else void Promise.resolve(onSave(change)).then(() => draft.discard(), e => setProblem(errorText(e)));
  };
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.defaultPrevented) return;
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && (e.target as HTMLElement).classList.contains("proposal-amount")) {
      e.preventDefault();
      save();
    }
  };
  return (
    <div className="proposal-editor" role="group" aria-label={t("Change {item}", { item: row.label })} onKeyDown={keys}>
      {row.edit?.category && (
        <CategoryPicker
          budget={budget}
          value={category}
          readyToAssign={t("Ready to assign")}
          label={t("Category for {item}", { item: row.label })}
          autoFocus
          onChange={setCategory}
        />
      )}
      {row.edit?.amount && (
        <input
          className="proposal-amount num-input"
          aria-label={t("Amount for {item}", { item: row.label })}
          value={amount}
          autoFocus={!row.edit.category}
          onFocus={(e) => e.target.select()}
          onChange={(e) => {
            setAmount(e.target.value);
            setProblem("");
          }}
        />
      )}
      {problem && <span className="proposal-editor-problem">{problem}</span>}
      <span className="proposal-editor-actions">
        <button type="button" className="primary small" disabled={busy} onClick={save}>
          {t("Save")}
        </button>
        <button type="button" className="secondary small" onClick={cancel}>
          {t("Cancel")}
        </button>
      </span>
    </div>
  );
}

/**
 * A change the assistant prepared. Nothing happens until Apply. While it waits, each part of it
 * can be left out (its checkbox) or given another category or amount (its pencil), and Apply
 * makes what's left; an applied card shrinks to one line with Undo while the change is still the
 * latest one.
 */
export function ProposalCard({
  budgetId,
  proposal: p,
  budget: book,
  canUndo,
  onApply,
  onDiscard,
  onUndo,
  onReview,
  onSave,
  onPreview,
  onPrint,
  onSheet,
  onRevise,
}: {
  budgetId?: string;
  /** Reports: open the preview, or go straight to the print dialog. */
  onPreview?: () => void;
  onPrint?: () => Promise<void>;
  /** Spreadsheets, and reports with tables: save the .xlsx file, or open it. */
  onSheet?: (open: boolean) => Promise<void>;
  proposal: Proposal;
  /** Its categories, to choose another from. */
  budget?: Pick<Budget, "categories" | "categoryOrder" | "groupOrder">;
  canUndo: boolean;
  onApply: () => Promise<void>;
  onDiscard: () => Promise<void>;
  onUndo: () => Promise<void>;
  onReview: () => void;
  onSave: () => Promise<void>;
  /** The card made again with the user's choices on it. */
  onRevise?: (revision: Revision) => Promise<void>;
}) {
  // Counted, so the first of two to finish doesn't offer Apply while the other is on its way.
  const [pending, setPending] = useState(0);
  const busy = pending > 0;
  const [editing, setEditing] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  // The choices as the revisions sent will leave them: each is made on the one before and sent
  // once that one is back, so one started before the last reply lands doesn't drop its change.
  const latest = useRef(p.choices ?? NONE);
  const queue = useRef(Promise.resolve());
  useLayoutEffect(() => {
    // Nothing on its way: the card's own (one that failed leaves them as they were).
    if (!pending) latest.current = p.choices ?? NONE;
  }, [p.choices, pending]);
  const run = (work: () => Promise<void>) => async () => {
    setPending((n) => n + 1);
    try {
      await work();
    } finally {
      setPending((n) => n - 1);
    }
  };
  const c = p.card;
  if (p.status === "applied" || p.status === "discarded" || p.status === "undone") {
    const label =
      p.status === "applied"
        ? t("Applied: {title}", { title: p.title })
        : p.status === "undone"
          ? t("Undone: {title}", { title: p.title })
          : t("Discarded: {title}", { title: p.title });
    return (
      <div className={`proposal done ${p.status}`}>
        {p.status === "applied" ? <Check size={14} aria-hidden /> : <Undo2 size={14} aria-hidden />}
        <span>{label}</span>
        {p.status === "applied" && canUndo && (
          <button className="secondary small" disabled={busy} onClick={run(onUndo)}>
            {t("Undo")}
          </button>
        )}
        {p.status === "undone" && (
          <button className="secondary small" disabled={busy} onClick={run(onApply)}>
            {t("Apply again")}
          </button>
        )}
      </div>
    );
  }
  const isImport = p.kind === "import";
  const isBudget = p.kind === "budget";
  // Its parts, to leave out or change, while it waits.
  const keyed = c.rows.filter((r) => r.key);
  const choosing = !!onRevise && p.status === "proposed" && keyed.length > 0;
  const chosen = keyed.filter((r) => !r.skipped).length;
  const revise = (make: (choices: Revision) => Revision) => {
    if (!onRevise) return;
    const revision = make(latest.current);
    latest.current = revision;
    const sent = queue.current.then(() => onRevise(revision));
    queue.current = sent.catch(() => {});
    void run(() => sent)();
    return sent;
  };
  const toggle = (row: CardRow) => {
    const key = row.key!;
    // An editor left open would save onto the card as it was.
    setEditing(null);
    revise((choices) =>
      row.skipped
        ? // Chosen again: an amount of nothing given to it goes too.
          { skip: choices.skip.filter((k) => k !== key), set: choices.set.filter((s) => !(s.key === key && s.amount === 0)) }
        : { skip: [...choices.skip, key], set: choices.set },
    );
  };
  const change = (row: CardRow, values: Omit<RowChange, "key">) => {
    const key = row.key!;
    setEditing(null);
    return revise((choices) => {
      const was = choices.set.find((s) => s.key === key);
      return { skip: choices.skip, set: [...choices.set.filter((s) => s.key !== key), { ...was, ...values, key }] };
    });
  };
  // A long card lists its first rows until asked for all (the one being changed stays in view).
  const shown = all ? c.rows : c.rows.filter((r, i) => i < SHOWN || (editing !== null && r.key === editing));
  const apply = p.kind === "fix" ? msg("Apply to the import") : msg("Apply");
  return (
    <div className={`proposal${p.status === "failed" ? " failed" : ""}`} aria-label={t("Proposal: {title}", { title: p.title })}>
      <div className="proposal-head">
        <span className="proposal-kicker">{c.kicker}</span>
        <strong>{p.title}</strong>
      </div>
      {c.stats.length > 0 && (
        <div className="proposal-stats">
          {c.stats.map((s) => (
            <div key={s.label}>
              <small>{s.label}</small>
              <b className="num">{s.text ?? (s.amount !== undefined ? money(s.amount) : "")}</b>
            </div>
          ))}
        </div>
      )}
      {shown.map((row, i) => (
        <div key={row.key ?? i}>
          <Row
            row={row}
            budget={isBudget}
            choosing={choosing}
            busy={busy}
            onToggle={() => toggle(row)}
            onEdit={() => setEditing(row.key ?? null)}
          />
          {choosing && editing !== null && row.key === editing && book && (
            <RowEditor
              budgetId={budgetId}
              row={row}
              budget={book}
              busy={busy}
              onSave={(values) => change(row, values)}
              onCancel={() => setEditing(null)}
            />
          )}
        </div>
      ))}
      {c.rows.length > SHOWN && (
        <button type="button" className="proposal-more link-button" onClick={() => setAll((a) => !a)}>
          {all ? t("Show fewer") : t("Show all {count}", { count: c.rows.length })}
        </button>
      )}
      {c.more > 0 && <div className="proposal-more">{tn(c.more, "and {count} more", "and {count} more")}</div>}
      {c.total && (
        <div className="proposal-total">
          <Row row={c.total} budget={isBudget} />
        </div>
      )}
      {c.check && (
        <div className={`proposal-check${c.check[0] ? " ok" : " bad"}`}>
          {c.check[0] ? <Check size={13} aria-hidden /> : <TriangleAlert size={13} aria-hidden />}
          {c.check[1]}
        </div>
      )}
      {(c.warning || p.problem) && (
        <div className="proposal-warning">
          <TriangleAlert size={13} aria-hidden />
          {p.problem ?? c.warning}
        </div>
      )}
      {c.note && <div className="proposal-note">{c.note}</div>}
      <div className="proposal-actions">
        {p.status === "failed" ? (
          <button className="secondary small" disabled={busy} onClick={run(onDiscard)}>
            {t("Dismiss")}
          </button>
        ) : p.kind === "report" ? (
          <>
            <button className="primary small" disabled={busy} onClick={onPreview}>
              <Eye size={13} aria-hidden /> {t("Preview")}
            </button>
            <button className="secondary small" disabled={busy} onClick={onPrint && run(onPrint)}>
              <FileDown size={13} aria-hidden /> {t("Save as PDF")}
            </button>
            {p.sheets > 0 && onSheet && (
              <button
                className="secondary small"
                disabled={busy}
                title={t("Each table on its own sheet ({count})", { count: p.sheets })}
                onClick={run(() => onSheet(false))}
              >
                <FileSpreadsheet size={13} aria-hidden /> Excel
              </button>
            )}
            <button className="link-button" disabled={busy} onClick={run(onDiscard)}>
              {t("Discard")}
            </button>
          </>
        ) : p.kind === "spreadsheet" ? (
          <>
            <button className="primary small" disabled={busy || !onSheet} onClick={onSheet && run(() => onSheet(true))}>
              {busy ? <LoaderCircle size={13} className="spin" aria-hidden /> : <FileSpreadsheet size={13} aria-hidden />} {t("Open")}
            </button>
            <button className="secondary small" disabled={busy || !onSheet} onClick={onSheet && run(() => onSheet(false))}>
              <FileDown size={13} aria-hidden /> {t("Save")}
            </button>
            <button className="link-button" disabled={busy} onClick={run(onDiscard)}>
              {t("Discard")}
            </button>
          </>
        ) : isImport ? (
          <>
            <button className="primary small" disabled={busy} onClick={onReview}>
              {t("Review import")}
            </button>
            <button className="secondary small" disabled={busy} onClick={run(onSave)}>
              <FileDown size={13} aria-hidden /> {t("Save as CSV")}
            </button>
            <button className="link-button" disabled={busy} onClick={run(onDiscard)}>
              {t("Discard")}
            </button>
          </>
        ) : (
          <>
            <button className="primary small" disabled={busy || (choosing && chosen === 0)} onClick={run(onApply)}>
              {busy ? (
                <LoaderCircle size={13} className="spin" aria-hidden />
              ) : p.problem && p.kind !== "fix" ? (
                <RefreshCw size={13} aria-hidden />
              ) : null}
              {choosing && chosen < keyed.length
                ? p.kind === "fix"
                  ? t("Apply to the import {chosen} of {total}", { chosen, total: keyed.length })
                  : t("Apply {chosen} of {total}", { chosen, total: keyed.length })
                : t(apply)}
            </button>
            <button className="secondary small" disabled={busy} onClick={run(onDiscard)}>
              {t("Discard")}
            </button>
            {p.choices ? (
              <button className="link-button proposal-hint" disabled={busy} onClick={() => revise(() => NONE)}>
                {t("Use the assistant's version")}
              </button>
            ) : (
              <span className="proposal-hint">
                {p.kind === "fix" ? t("Each applies only to a line that hasn't changed") : t("One undo step")}
              </span>
            )}
          </>
        )}
      </div>
    </div>
  );
}
