import { useEditorCheckpoint } from "../editorCheckpoint";
import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import {
  Check,
  ChevronDown,
  CircleAlert,
  Eye,
  FileDown,
  FileSpreadsheet,
  Info,
  LoaderCircle,
  MoreHorizontal,
  Pencil,
  TriangleAlert,
  Undo2,
} from "lucide-react";
import { editableAmount, errorText, evaluateAmount, money } from "../format";
import type { Budget } from "../types";
import { CategoryPicker, Menu, READY_TO_ASSIGN } from "../ui";
import type { CardMessage, CardRow, Proposal, Revision } from "./api";
import { localDateTime, ltr, t, tc, tn } from "../i18n";

// The sign stays with the amount in Arabic too.
const signed = (n: number) => (n > 0 ? ltr(`+${money(n)}`) : money(n));
/** Rows a card lists before "Show all". */
const SHOWN = 8;
/** A card with more rows to choose among than this offers to select them all, or none. */
const MANY = 5;
/** How long after the last tick or change a card's choices go to the core (milliseconds). */
export const SETTLE = 600;
/** The kinds this app draws. Any other is drawn plain, with a line saying to update. */
const KINDS = new Set([
  "budget",
  "transactions",
  "payees",
  "notes",
  "categories",
  "schedules",
  "delete",
  "fix",
  "bankTexts",
  "report",
  "spreadsheet",
  "import",
  "figures",
  "question",
]);

type RowChange = Revision["set"][number];
const NONE: Revision = { skip: [], set: [] };

/** What a card's buttons do. The card is busy until the ones that take a while are done. */
export type CardHandlers = {
  /** Applies the change. `now` is the card as the core just made it with the user's choices, when some were waiting to be sent. */
  apply: (now?: Proposal) => Promise<void>;
  /** The card made again with the user's choices on it. It fails when the core doesn't take them. */
  revise: (revision: Revision) => Promise<Proposal>;
  /** Sets the card aside. A thing the assistant made is removed the same way. */
  discard: () => Promise<void>;
  /** Puts back a card that was set aside, removed or replaced. */
  restore: () => Promise<void>;
  undo: () => Promise<void>;
  /** Puts a line about the card in the message field, for the user to finish. */
  askChanges: () => void;
  /** Asks the assistant to prepare the change again. */
  askAgain: () => void;
  /** Goes to what the card names in the budget. */
  show: () => void;
  /** A question's answer, sent as the user's message. */
  answer: (label: string) => void;
  /** A converted file: open its import review, or save it as a CSV. */
  review: () => void;
  saveCsv: () => Promise<void>;
  /** Reports: open the preview, or go straight to the print dialog. */
  preview: () => void;
  print: () => Promise<void>;
  /** Spreadsheets, and reports with tables: save the .xlsx file, or open it. */
  sheet: (open: boolean) => Promise<void>;
};

const Row = memo(function Row({
  row,
  budget,
  choosing,
  busy,
  share,
  onToggle,
  onEdit,
}: {
  row: CardRow;
  budget: boolean;
  /** Its checkbox and editing are offered: the card is waiting to be applied. */
  choosing?: boolean;
  busy?: boolean;
  /** Figures: its amount as a share of the card's largest, drawn as a bar under it. */
  share?: number;
  onToggle?: (row: CardRow) => void;
  onEdit?: (row: CardRow) => void;
}) {
  const editable = choosing && !row.skipped && (row.edit?.category || row.edit?.amount);
  return (
    <>
      <div
        className={`proposal-row${row.tone === "warn" ? " warn" : ""}${row.skipped ? " skipped" : ""}${row.edited ? " edited" : ""}`}
        data-row={row.key}
      >
        {choosing && row.key && (
          <input
            type="checkbox"
            className="proposal-pick"
            checked={!row.skipped}
            disabled={busy}
            aria-label={t("Apply: {item}", { item: row.label })}
            onChange={() => onToggle?.(row)}
          />
        )}
        {/* A payee or a category reads in its own direction, whichever way the app is laid out. */}
        <span className="proposal-label">
          <span dir="auto" title={row.label}>
            {row.label}
          </span>
          {row.detail && (
            <small dir="auto" title={row.detail}>
              {row.detail}
            </small>
          )}
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
            // Its editor opens under the row: the conversation doesn't scroll for that (see useFollow).
            data-grows
            disabled={busy}
            aria-label={t("Change {item}", { item: row.label })}
            title={
              row.edit?.category && row.edit.amount
                ? t("Change the category or amount")
                : row.edit?.category
                  ? t("Change the category")
                  : t("Change the amount")
            }
            onClick={() => onEdit?.(row)}
          >
            <Pencil size={12} aria-hidden />
          </button>
        )}
      </div>
      {/* Bars run left to right in Arabic too, as the app's charts do. */}
      {share !== undefined && (
        <div className="proposal-bar" dir="ltr" aria-hidden>
          <i style={{ width: `${Math.round(share * 100)}%` }} />
        </div>
      )}
    </>
  );
});

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
  /** The card is being applied: what's saved now would come too late for it. */
  busy?: boolean;
  onSave: (change: Omit<RowChange, "key">) => void;
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
    else {
      draft.discard();
      onSave(change);
    }
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

/** What the card has to say about itself, by its tone: quiet, a warning, or an error. */
function Message({ message }: { message: CardMessage }) {
  return (
    <div className={`proposal-message ${message.tone}`}>
      {message.tone === "error" ? (
        <CircleAlert size={13} aria-hidden />
      ) : message.tone === "warn" ? (
        <TriangleAlert size={13} aria-hidden />
      ) : (
        <Info size={13} aria-hidden />
      )}
      <span>{message.text}</span>
    </div>
  );
}

/** When a card was applied, as the app writes a date and time; nothing when it can't be read. */
function appliedTime(at: string) {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "" : localDateTime(date, { dateStyle: "medium", timeStyle: "short" });
}

/**
 * A card the assistant made: a change it prepared, a thing it made (a report, a spreadsheet, a
 * converted file, figures) or a question. The core says what the card offers (`actions`) and
 * what it has to say (`message`); the card draws those, and never works them out itself.
 *
 * Nothing happens until Apply. While a change waits, each part of it can be left out (its
 * checkbox) or given another category or amount (its pencil): that shows at once, and goes to
 * the core a moment after the last change, and before Apply. A card that's finished (applied,
 * set aside, undone, replaced) folds to one line, which opens to its rows as they were.
 *
 * Drawn again only when what it's given changes: the panel hands it the same functions for as
 * long as the proposal is the same, so typing a message or a reply streaming in leaves it alone.
 */
export const ProposalCard = memo(function ProposalCard({
  budgetId,
  proposal: p,
  budget: book,
  canUndo,
  fixable = true,
  answerable = true,
  on,
}: {
  budgetId?: string;
  proposal: Proposal;
  /** Its categories, to choose another from. */
  budget?: Pick<Budget, "categories" | "categoryOrder" | "groupOrder">;
  /** The app can still take its change back: it is the budget's latest. */
  canUndo: boolean;
  /** The import its fixes are for is open. */
  fixable?: boolean;
  /** A message can be sent now, so a question's answers can be pressed. */
  answerable?: boolean;
  on: CardHandlers;
}) {
  // Counted, so the first of two to finish doesn't offer Apply while the other is on its way.
  const [pending, setPending] = useState(0);
  const busy = pending > 0;
  const [editing, setEditing] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  /** A finished card's rows are showing. */
  const [open, setOpen] = useState(false);
  const [menu, setMenu] = useState(false);
  const card = useRef<HTMLDivElement>(null);
  const more = useRef<HTMLButtonElement>(null);
  /** The row whose editor just closed: the keyboard goes back to its pencil. */
  const closed = useRef<string | null>(null);
  const closeEditor = () => {
    closed.current = editing;
    setEditing(null);
  };
  // The choices made here that the core hasn't answered for yet. The rows show them at once;
  // `waiting` is the same, readable before the card is drawn again.
  const [local, setLocal] = useState<Revision | null>(null);
  const waiting = useRef<Revision | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const sending = useRef<Promise<Proposal | undefined> | null>(null);
  const handlers = useRef(on);
  handlers.current = on;
  const kept = useRef(p.choices);
  kept.current = p.choices;

  /**
   * Sends the choices waiting here to the core, one call at a time, and resolves to the card it
   * made of them (nothing, when none were waiting). Choices made while a call is on its way go
   * in the next one: each is the whole of what's chosen, so none is lost.
   */
  const send = () => {
    clearTimeout(timer.current);
    timer.current = undefined;
    const before = sending.current;
    const sent = (async () => {
      const earlier = before ? await before.then((made) => ({ made }), (error: unknown) => ({ error })) : { made: undefined };
      const revision = waiting.current;
      if (!revision) {
        if ("error" in earlier) throw earlier.error;
        return earlier.made;
      }
      try {
        return await handlers.current.revise(revision);
      } finally {
        // Answered or refused, the card shows what the core has, unless more was chosen meanwhile.
        if (waiting.current === revision) {
          waiting.current = null;
          setLocal(null);
        }
      }
    })();
    sending.current = sent;
    const done = () => {
      if (sending.current === sent) sending.current = null;
    };
    sent.then(done, done);
    return sent;
  };
  /** A choice made on the card: shown at once, and sent a moment after the last one (or `now`). */
  const choose = (make: (choices: Revision) => Revision, now = false) => {
    const next = make(waiting.current ?? kept.current ?? NONE);
    waiting.current = next;
    setLocal(next);
    clearTimeout(timer.current);
    // Whoever is told of a refusal is the panel; the card goes back to what the core has.
    if (now) void send().catch(() => {});
    else timer.current = setTimeout(() => void send().catch(() => {}), SETTLE);
  };
  // Closing the panel doesn't lose a tick made a moment before.
  useEffect(
    () => () => {
      if (timer.current !== undefined) void send().catch(() => {});
    },
    [],
  );

  const c = p.card;
  const offers = (action: string) => p.actions.includes(action);
  const known = KINDS.has(p.kind);
  const finished = p.status === "applied" || p.status === "discarded" || p.status === "undone" || p.status === "replaced";
  const asking = p.kind === "question" && offers("answer") && !finished;
  /** A question that has its answer: what was asked is all that's left to say of it. */
  const asked = p.kind === "question" && !asking && (p.status === "applied" || p.status === "proposed");
  const folded = finished || asked;
  const isBudget = p.kind === "budget";
  // Its parts, to leave out or change, while it offers to apply them.
  const keyed = c.rows.filter((r) => r.key);
  const choosing = known && !folded && (offers("apply") || offers("applyToImport")) && keyed.length > 0;
  useEffect(() => {
    // No longer waiting (applied, set aside, replaced): what was chosen here has nothing to go to.
    if (choosing) return;
    clearTimeout(timer.current);
    timer.current = undefined;
    waiting.current = null;
    setLocal(null);
  }, [choosing]);
  useLayoutEffect(() => {
    // The keyboard goes back to the pencil of the row whose editor closed, unless it has gone
    // somewhere else meanwhile.
    if (editing !== null || closed.current === null || pending) return;
    const key = closed.current;
    closed.current = null;
    const active = document.activeElement;
    if (active && active !== document.body && !card.current?.contains(active)) return;
    const pencil = [...(card.current?.querySelectorAll<HTMLElement>(".proposal-row") ?? [])]
      .find((row) => row.dataset.row === key)
      ?.querySelector<HTMLElement>(".proposal-edit");
    pencil?.focus();
  }, [editing, pending]);

  const run = (work: () => Promise<void>) => async () => {
    setPending((n) => n + 1);
    try {
      await work();
    } catch {
      // The panel has said what went wrong, and shows the card as the core has it.
    } finally {
      setPending((n) => n - 1);
    }
  };

  /** A row as the choices made here leave it, until the core's card says the same. */
  const shownAs = (row: CardRow): CardRow => {
    if (!local || !row.key) return row;
    const set = local.set.find((s) => s.key === row.key);
    const skipped = local.skip.includes(row.key) || set?.amount === 0;
    if (!set) return !!row.skipped === skipped ? row : { ...row, skipped };
    const moved = set.category !== undefined && set.category !== (row.edit?.categoryId ?? null);
    return {
      ...row,
      skipped,
      edited: true,
      amount: set.amount ?? row.amount,
      // The category it gives now, by name, until the core words the row again.
      detail: moved ? (set.category === null ? t("Ready to assign") : (book?.categories.find((x) => x.id === set.category)?.name ?? row.detail)) : row.detail,
      edit: row.edit && set.category !== undefined ? { ...row.edit, categoryId: set.category ?? undefined } : row.edit,
    };
  };
  const rows = local ? c.rows.map(shownAs) : c.rows;
  const chosen = rows.filter((r) => r.key && !r.skipped).length;
  const partial = choosing && chosen < keyed.length;

  const toggle = (row: CardRow) => {
    const key = row.key!;
    // An editor left open would save onto the row as it was.
    setEditing(null);
    choose((choices) =>
      row.skipped
        ? // Chosen again: an amount of nothing given to it goes too.
          { skip: choices.skip.filter((k) => k !== key), set: choices.set.filter((s) => !(s.key === key && s.amount === 0)) }
        : { skip: [...choices.skip.filter((k) => k !== key), key], set: choices.set },
    );
  };
  const change = (row: CardRow, values: Omit<RowChange, "key">) => {
    const key = row.key!;
    closeEditor();
    choose((choices) => {
      const was = choices.set.find((s) => s.key === key);
      return { skip: choices.skip, set: [...choices.set.filter((s) => s.key !== key), { ...was, ...values, key }] };
    });
  };
  /** Chooses the rows that are `wanted`, and leaves out the rest. */
  const pick = (wanted: (row: CardRow) => boolean) => {
    setEditing(null);
    choose((choices) => ({
      skip: keyed.filter((r) => !wanted(r)).map((r) => r.key!),
      set: choices.set.filter((s) => !(s.amount === 0 && keyed.some((r) => r.key === s.key && wanted(r)))),
    }));
  };
  // The rows are handed the same two functions for the card's whole life, so a tick draws
  // again only the row it changes.
  const acts = useRef({ toggle, edit: (row: CardRow) => setEditing(row.key ?? null) });
  acts.current = { toggle, edit: (row: CardRow) => setEditing(row.key ?? null) };
  const onToggle = useCallback((row: CardRow) => acts.current.toggle(row), []);
  const onEdit = useCallback((row: CardRow) => acts.current.edit(row), []);

  const apply = async () => {
    // The choices made here go first: what's applied is the card as the core then has it.
    const now = waiting.current || sending.current ? await send() : undefined;
    await on.apply(now);
  };

  // A long card lists its first rows until asked for all (the one being changed stays in view).
  const listed = all ? rows : rows.filter((r, i) => i < SHOWN || (editing !== null && r.key === editing));
  // Figures: each row's bar is its share of the largest amount on the card.
  const largest = p.kind === "figures" ? Math.max(0, ...c.rows.map((r) => Math.abs(r.amount ?? 0))) : 0;
  const share = (row: CardRow) => (p.kind === "figures" && row.amount !== undefined ? (largest ? Math.abs(row.amount) / largest : 0) : undefined);

  const body = (
    <>
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
      {choosing && keyed.length > MANY && (
        <div className="proposal-select">
          <button type="button" className="link-button" disabled={busy || chosen === keyed.length} onClick={() => pick(() => true)}>
            {t("Select all")}
          </button>
          <button type="button" className="link-button" disabled={busy || chosen === 0} onClick={() => pick(() => false)}>
            {t("Select none")}
          </button>
          {keyed.some((r) => r.tone === "warn") && (
            <button type="button" className="link-button" disabled={busy} onClick={() => pick((r) => r.tone === "warn")}>
              {t("Only the ones to check")}
            </button>
          )}
        </div>
      )}
      {asking ? (
        // A question: each answer is a button, and pressing one sends it as the user's message.
        <div className="proposal-answers">
          {c.rows.map((row, i) => (
            <button key={i} type="button" className="proposal-answer" disabled={busy || !answerable} onClick={() => on.answer(row.label)}>
              <span dir="auto">{row.label}</span>
              {row.detail && <small dir="auto">{row.detail}</small>}
            </button>
          ))}
        </div>
      ) : (
        listed.map((row, i) => (
          <div key={row.key ?? i}>
            <Row row={row} budget={isBudget} choosing={choosing} busy={busy} share={share(row)} onToggle={onToggle} onEdit={onEdit} />
            {choosing && editing !== null && row.key === editing && book && (
              <RowEditor budgetId={budgetId} row={row} budget={book} busy={busy} onSave={(values) => change(row, values)} onCancel={closeEditor} />
            )}
          </div>
        ))
      )}
      {!asking && c.rows.length > SHOWN && (
        <button type="button" className="proposal-more link-button" data-grows aria-expanded={all} onClick={() => setAll((a) => !a)}>
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
    </>
  );
  // What the card warns of is part of what it proposes; what it says of itself (`message`) is apart.
  const warning = c.warning && (
    <div className="proposal-warning">
      <TriangleAlert size={13} aria-hidden />
      {c.warning}
    </div>
  );
  const note = c.note && <div className="proposal-note">{c.note}</div>;

  /** The words on the button that applies the card. */
  const applyLabel = (action: string) => {
    if (p.kind === "delete") {
      const count = keyed.length ? chosen : c.rows.length + c.more;
      return tn(count, "Delete {count} transaction", "Delete {count} transactions");
    }
    if (action === "applyToImport")
      return partial ? t("Apply to the import {chosen} of {total}", { chosen, total: keyed.length }) : t("Apply to the import");
    if (partial) return t("Apply {chosen} of {total}", { chosen, total: keyed.length });
    return p.status === "undone" ? t("Apply again") : t("Apply");
  };
  /**
   * The button for an action the core offers, or nothing for one that isn't a button here:
   * `remove` is in the card's menu, `answer` is the question's rows, `undo` shows only while the
   * app can, and one this app doesn't know is left out. `main` is the card's main button.
   */
  const button = (action: string, main: boolean): ReactNode => {
    const look = main ? "primary small" : "secondary small";
    const icon = (usual: ReactNode) => (busy && main ? <LoaderCircle size={13} className="spin" aria-hidden /> : usual);
    switch (action) {
      case "apply":
      case "applyToImport": {
        const closedImport = action === "applyToImport" && !fixable;
        return (
          <button
            key={action}
            className={p.kind === "delete" ? "danger small" : look}
            disabled={busy || closedImport || (choosing && chosen === 0)}
            title={closedImport ? t("Open the import these fixes are for.") : undefined}
            onClick={run(apply)}
          >
            {icon(null)}
            {applyLabel(action)}
          </button>
        );
      }
      case "askChanges":
        return (
          <button key={action} className={look} disabled={busy} onClick={on.askChanges}>
            {t("Ask for changes")}
          </button>
        );
      case "useOriginal":
        return (
          <button key={action} className="link-button" disabled={busy} onClick={() => choose(() => NONE, true)}>
            {t("Use the assistant's version")}
          </button>
        );
      case "discard":
        // Apart from the main button, at the row's far end: not one to press by mistake.
        return (
          <button key={action} className="link-button proposal-discard" disabled={busy} onClick={run(on.discard)}>
            {t("Discard")}
          </button>
        );
      case "restore":
        return (
          <button key={action} className={look} disabled={busy} onClick={run(on.restore)}>
            {icon(null)}
            {tc("card", "Restore")}
          </button>
        );
      case "undo":
        return canUndo ? (
          <button key={action} className={look} disabled={busy} onClick={run(on.undo)}>
            {icon(null)}
            {t("Undo")}
          </button>
        ) : null;
      case "askAgain":
        return (
          <button key={action} className={look} disabled={busy} onClick={on.askAgain}>
            {t("Ask for a new version")}
          </button>
        );
      case "show":
        return (
          <button key={action} className={look} disabled={busy} onClick={on.show}>
            {t("Show in the budget")}
          </button>
        );
      case "preview":
        return (
          <button key={action} className={look} disabled={busy} onClick={on.preview}>
            <Eye size={13} aria-hidden /> {t("Preview")}
          </button>
        );
      case "print":
        return (
          <button key={action} className={look} disabled={busy} onClick={run(on.print)}>
            {icon(<FileDown size={13} aria-hidden />)} {t("Print or save as PDF")}
          </button>
        );
      case "excel":
        return (
          <button
            key={action}
            className={look}
            disabled={busy}
            title={t("Each table on its own sheet ({count})", { count: p.sheets })}
            onClick={run(() => on.sheet(false))}
          >
            {icon(<FileSpreadsheet size={13} aria-hidden />)} Excel
          </button>
        );
      case "open":
        return (
          <button key={action} className={look} disabled={busy} onClick={run(() => on.sheet(true))}>
            {icon(<FileSpreadsheet size={13} aria-hidden />)} {t("Open")}
          </button>
        );
      case "save":
        return (
          <button key={action} className={look} disabled={busy} onClick={run(() => on.sheet(false))}>
            {icon(<FileDown size={13} aria-hidden />)} {t("Save")}
          </button>
        );
      case "reviewImport":
        return (
          <button key={action} className={look} disabled={busy} onClick={on.review}>
            {t("Review import")}
          </button>
        );
      case "saveCsv":
        return (
          <button key={action} className={look} disabled={busy} onClick={run(on.saveCsv)}>
            {icon(<FileDown size={13} aria-hidden />)} {t("Save as CSV")}
          </button>
        );
      default:
        return null;
    }
  };
  /** The buttons for these actions, in the order given; the first that is one is the main button when `lead`. */
  const buttons = (actions: string[], lead: boolean) => {
    let main = lead;
    return actions.map((action) => {
      const made = button(action, main);
      if (made) main = false;
      return made;
    });
  };

  if (folded) {
    const time = p.status === "applied" && p.appliedAt ? appliedTime(p.appliedAt) : "";
    const state =
      p.status === "applied"
        ? time
          ? t("Applied {time}", { time })
          : t("Applied")
        : p.status === "undone"
          ? t("Undone")
          : p.status === "replaced"
            ? t("Replaced by a newer version")
            : p.made
              ? t("Removed")
              : t("Discarded");
    const line = (
      <>
        {p.status === "applied" || asked ? <Check size={14} aria-hidden /> : <Undo2 size={14} aria-hidden />}
        {asked ? (
          // An answered question: what was asked is the whole of the line.
          <span className="proposal-done-title" dir="auto" title={p.title}>
            {t("Asked: {question}", { question: p.title })}
          </span>
        ) : (
          <>
            <span className="proposal-state">{state}</span>
            <span className="proposal-done-title" dir="auto" title={p.title}>
              {p.title}
            </span>
          </>
        )}
      </>
    );
    const details = c.rows.length > 0 || c.stats.length > 0 || !!c.total || !!c.check || !!c.warning || !!c.note;
    // On the line: what's done with a finished card (take it back, put it back, go to it, apply
    // it again). What changes it or sets it aside waits under its rows.
    const under = ["askChanges", "useOriginal", "discard"];
    const below = buttons(p.actions.filter((a) => under.includes(a)), false).filter(Boolean);
    return (
      <div ref={card} className={`proposal done ${p.status}${p.kind === "delete" ? " destructive" : ""}`} aria-label={t("Proposal: {title}", { title: p.title })}>
        <div className="proposal-done-line">
          {details ? (
            // Opening it is the user's doing: the conversation doesn't scroll for it (see useFollow).
            <button type="button" className="proposal-fold" data-grows aria-expanded={open} onClick={() => setOpen((o) => !o)}>
              {line}
              <ChevronDown size={14} aria-hidden className="proposal-chevron" />
            </button>
          ) : (
            <span className="proposal-fold">{line}</span>
          )}
          {buttons(p.actions.filter((a) => !under.includes(a)), false)}
        </div>
        {p.message && <Message message={p.message} />}
        {open && details && (
          <div className="proposal-details">
            {body}
            {warning}
            {note}
            {below.length > 0 && <div className="proposal-actions">{below}</div>}
          </div>
        )}
      </div>
    );
  }

  const menuItems = offers("remove") ? [{ label: t("Remove"), disabled: busy, onSelect: () => void run(on.discard)() }] : [];
  const row = buttons(p.actions, true);
  const hint =
    offers("applyToImport") && !fixable
      ? t("Open the import these fixes are for.")
      : offers("applyToImport")
        ? t("Each applies only to a line that hasn't changed")
        : offers("apply") && known
          ? t("One undo step")
          : "";
  return (
    <div
      ref={card}
      className={`proposal${p.status === "failed" ? " failed" : ""}${p.kind === "delete" ? " destructive" : ""}`}
      aria-label={t("Proposal: {title}", { title: p.title })}
    >
      <div className="proposal-head">
        <div className="proposal-titles">
          <span className="proposal-kicker">{c.kicker}</span>
          <strong dir="auto">{p.title}</strong>
        </div>
        {menuItems.length > 0 && (
          <>
            <button
              ref={more}
              type="button"
              className="icon-button proposal-menu"
              aria-label={t("More actions")}
              title={t("More actions")}
              aria-expanded={menu}
              onClick={() => setMenu((m) => !m)}
            >
              <MoreHorizontal size={15} />
            </button>
            <Menu open={menu} onClose={() => setMenu(false)} anchor={more} placement="bottom-end" label={t("More actions")} items={menuItems} />
          </>
        )}
      </div>
      {body}
      {p.message && <Message message={p.message} />}
      {warning}
      {note}
      {!known && <div className="proposal-note">{t("Update UrBudget to use this card.")}</div>}
      {row.some(Boolean) && (
        <div className="proposal-actions">
          {row}
          {hint && <span className="proposal-hint">{hint}</span>}
        </div>
      )}
    </div>
  );
});
