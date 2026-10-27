import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Lock, MoreHorizontal, Plus, Repeat, Split as SplitIcon, X } from "lucide-react";
import { CategoryPicker, DateInput, PayeeInput, READY_TO_ASSIGN, transferPayee } from "./ui";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import { id, scheduledAhead, today, type Budget, type Split, type Status, type Transaction } from "./types";
import { getPref } from "./prefs";
import { uncategorizedTest } from "./uncategorized";
import { frequencies } from "./Planning";
import { msg, t, tn } from "./i18n";

/** The flags and their colors' names (translate a name with t() when showing it). */
export const FLAGS: [string, string][] = [
  ["red", msg("Red")],
  ["orange", msg("Orange")],
  ["yellow", msg("Yellow")],
  ["green", msg("Green")],
  ["blue", msg("Blue")],
  ["purple", msg("Purple")],
];
/** A flag's name: what this budget calls it, or its color in the language in use. */
export const flagName = (flag: string, b?: Pick<Budget, "flagNames"> | null) =>
  b?.flagNames?.[flag]?.trim() || t(FLAGS.find(([f]) => f === flag)?.[1] ?? flag);
/** The flags in order, each with this budget's name for it. */
export const flagsOf = (b?: Pick<Budget, "flagNames"> | null): [string, string][] =>
  FLAGS.map(([f]) => [f, flagName(f, b)]);
/**
 * A flag drawn in its color: a banner with a notched tail, a third wider than it is tall (`size`
 * is its height). Without a flag it takes the text's color. `outline` draws its edge only, for
 * "no flag yet" and the Flag heading.
 */
export const FlagIcon = ({ flag, size = 12, outline = false }: { flag?: string; size?: number; outline?: boolean }) => (
  <svg
    className="flag-icon"
    data-flag={flag}
    width={Math.round((size * 4) / 3)}
    height={size}
    viewBox={outline ? "-0.5 -0.5 17 13" : "0 0 16 12"}
    aria-hidden
  >
    <path
      d="M.75 2.25A2 2 0 0 1 2.75.25h11.9a.75.75 0 0 1 .62 1.17L12.35 6l2.92 4.58a.75.75 0 0 1-.62 1.17H2.75a2 2 0 0 1-2-2z"
      fill={outline ? "none" : "currentColor"}
      stroke={outline ? "currentColor" : undefined}
      strokeWidth={outline ? 1.4 : undefined}
      strokeLinejoin="round"
    />
  </svg>
);
/** A flag as a label tinted in its color, with its name. */
export const FlagTag = ({ flag, name }: { flag: string; name: string }) => (
  <span className="flag-tag" data-flag={flag}>
    <FlagIcon flag={flag} size={11} />
    <span>{name}</span>
  </span>
);

/**
 * A transaction's cleared state: a dashed ring while uncleared, a solid check once cleared, a
 * lock once reconciled. Drawn as SVG so it sits exactly in the middle at any size.
 */
export function ClearedMark({ status }: { status: Status }) {
  if (status === "reconciled")
    return (
      <span className="status-icon reconciled" aria-hidden>
        <Lock size={10} strokeWidth={2.5} />
      </span>
    );
  return (
    <svg className={`status-icon ${status}`} viewBox="0 0 18 18" width={18} height={18} aria-hidden>
      {status === "cleared" ? <circle cx="9" cy="9" r="8" className="disc" /> : <circle cx="9" cy="9" r="7.25" className="ring" />}
      <path d="M5.6 9.3l2.2 2.2 4.6-4.8" className="tick" />
    </svg>
  );
}

/**
 * Whether a register row can be edited in place: ordinary transactions and splits, and
 * transfers (from either side) with at most one category. Split lines that are themselves
 * transfers still need the full dialog.
 */
export const inlineEditable = (t: Transaction, reverse = false, part?: number) => {
  if (part !== undefined || t.splits.some((s) => s.transferAccountId)) return false;
  if (t.transferAccountId) return t.splits.length <= 1;
  return !reverse;
};
/** New transactions start cleared or not, as set in Settings › Transactions. */
const newStatus = (): Status => (getPref("newCleared") ? "cleared" : "uncleared");
const cents = (n: number) => (n === 0 ? "" : editableAmount(Math.abs(n)));
/** The editor's fields, by the label of their input. */
export type EditorField = "Date" | "Payee" | "Category" | "Memo" | "Outflow" | "Inflow";
type Line = { categoryId: string | null; memo: string; outflow: string; inflow: string };
/** A saved line for editing. With no category it opens on Unassigned, unless it still `needs` one. */
const lineOf = (s: Split, needs: boolean): Line => ({
  categoryId: s.categoryId ?? (needs ? null : READY_TO_ASSIGN),
  memo: s.memo ?? "",
  outflow: s.amount < 0 ? cents(s.amount) : "",
  inflow: s.amount > 0 ? cents(s.amount) : "",
});
const amountOf = (l: { outflow: string; inflow: string }) => evaluateAmount(l.inflow) - evaluateAmount(l.outflow);
/** Money in (an inflow typed, no outflow): its "no category" is income for Unassigned. */
const isIncome = (l: { outflow: string; inflow: string }) => !!l.inflow.trim() && !l.outflow.trim();
/** The picker's Unassigned, as the register says it: income goes to it. Spending isn't offered it,
 * as YNAB steers spending to a category; one already taken from it (a YNAB import, a deleted
 * category) still shows as that until it's given a category. */
export const INCOME = msg("To Unassigned");
export const FROM_UNASSIGNED = msg("From Unassigned");
const safeAmount = (l: { outflow: string; inflow: string }) => {
  try {
    return amountOf(l);
  } catch {
    return NaN;
  }
};

/**
 * The register's add/edit row. Enter saves, Escape cancels, Tab moves through the fields.
 * Choosing a payee fills in the category last used with it; choosing "Transfer: <account>"
 * makes it a transfer. "Split" divides it into lines that must add up to the total.
 */
export function TransactionEditor({
  budget,
  accountId,
  initial,
  reverse = false,
  available,
  focus,
  onSave,
  onCancel,
  onMore,
  onLines,
  onCreateCategory,
}: {
  budget: Budget;
  /** The register's account, or "all" to choose one per transaction. */
  accountId: string;
  initial?: Transaction;
  /** `initial` is a transfer seen from its receiving account (this register's). */
  reverse?: boolean;
  available?: Map<string, number>;
  /** The field to start in; new rows start at Payee, edits at Date. */
  focus?: EditorField;
  /** `repeat` is a schedule frequency, when a new transaction is to repeat (saveTransaction). */
  onSave: (t: Transaction, repeat?: string) => Promise<void>;
  onCancel: () => void;
  /** Opens the full dialog with the transaction and any repeat chosen for a new draft. */
  onMore: (draft: Transaction, repeat?: string) => void;
  /** Reports how many split lines are showing, so the register can size the row. */
  onLines?: (count: number) => void;
  /** Creates a category named in a category field; resolves to its id, or null if cancelled. */
  onCreateCategory?: (name: string) => Promise<string | null>;
}) {
  const all = accountId === "all";
  const accounts = budget.accounts.filter((a) => !a.archived || a.id === initial?.accountId);
  const names = useMemo(() => new Map(budget.accounts.map((a) => [a.id, a.name])), [budget.accounts]);
  const kindOf = (accountId: string | null) => budget.accounts.find((a) => a.id === accountId)?.kind;

  const startTransfer = initial ? (reverse ? initial.accountId : initial.transferAccountId) : null;
  const startAmount = initial ? (reverse ? -initial.amount : initial.amount) : 0;
  const [draftId, setDraftId] = useState(() => initial?.id ?? id());
  const [account, setAccount] = useState(
    reverse ? accountId : (initial?.accountId ?? (all ? (accounts[0]?.id ?? "") : accountId)),
  );
  const [date, setDate] = useState(initial?.date ?? today());
  const [transfer, setTransfer] = useState<string | null>(startTransfer);
  const [payee, setPayee] = useState(
    startTransfer ? transferPayee(names.get(startTransfer) ?? "") : (initial?.payee ?? ""),
  );
  // Spending saved without a category opens with none chosen, not on Unassigned (whatever its
  // date: a scheduled one needs one as much).
  const needs = useMemo(() => uncategorizedTest(budget, "9999-12-31"), [budget.accounts, budget.categories]);
  const [category, setCategory] = useState<string | null>(() => {
    const first = initial?.splits[0];
    return !initial ? null : (first?.categoryId ?? (first && needs(initial, first) ? null : READY_TO_ASSIGN));
  });
  const touchedCategory = useRef(!!initial);
  const [memo, setMemo] = useState(
    initial ? (reverse ? String(initial.transferSource?.memo ?? initial.memo ?? "") : initial.memo) : "",
  );
  const [flag, setFlag] = useState(
    String((initial ? (reverse ? initial.transferSource?.flag_color : initial.flag) : "") ?? ""),
  );
  const [outflow, setOutflow] = useState(startAmount < 0 ? cents(startAmount) : "");
  const [inflow, setInflow] = useState(startAmount > 0 ? cents(startAmount) : "");
  // Until an inflow is typed it's taken to be spending: the category comes before the amount.
  const income = isIncome({ outflow, inflow });
  const [status, setStatus] = useState<Status>(
    initial ? (reverse ? (initial.transferStatus ?? "uncleared") : initial.status) : newStatus(),
  );
  const [lines, setLines] = useState<Line[] | null>(
    initial && !startTransfer && initial.splits.length > 1 ? initial.splits.map((s) => lineOf(s, needs(initial, s))) : null,
  );
  // A new transaction can repeat: it's scheduled, and entered now unless it's dated ahead.
  const [repeat, setRepeat] = useState("never");
  // Dated ahead and not yet cleared, it's a scheduled transaction, entered on its date
  // (scheduledAhead, saveTransaction).
  const ahead = date > today() && status === "uncleared" &&
    (!initial || scheduledAhead({ ...initial, date, ...(reverse ? { transferStatus: status } : { status }) }));
  // Nothing dated ahead has cleared yet: a new one dated ahead starts uncleared, whatever
  // Settings says, until its cleared mark is set by hand.
  const statusSet = useRef(false);
  const pickDate = (next: string) => {
    setDate(next);
    if (!initial && !statusSet.current) setStatus(next > today() ? "uncleared" : newStatus());
  };
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const row = useRef<HTMLDivElement>(null);
  const tracking = kindOf(account) === "tracking";
  // A transfer needs a category only when it moves money in or out of the budget.
  const needsCategory = transfer ? tracking !== (kindOf(transfer) === "tracking") : !tracking;
  const payment = !!budget.categories.find((c) => c.id === category)?.paymentAccountId;
  const transfers = useMemo(
    () => budget.accounts.filter((a) => !a.archived && a.id !== account).map((a) => ({ id: a.id, name: a.name })),
    [budget.accounts, account],
  );

  // Fields are found by their (translated) labels.
  const focusLabel = (label: string) =>
    requestAnimationFrame(() => row.current?.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)?.focus());
  const focusField = (field: EditorField) => focusLabel(t(field));
  useEffect(() => {
    focusField(focus ?? (initial ? "Date" : "Payee"));
  }, []);
  useLayoutEffect(() => {
    onLines?.(lines?.length ?? 0);
  }, [lines?.length]);

  const total = () => amountOf({ outflow, inflow });
  /** A draft for the dialog keeps READY_TO_ASSIGN, so null there still means "not chosen". */
  const build = (amount: number, draft = false): Transaction => {
    const categoryOf = (c: string | null) => (draft ? c : !c || c === READY_TO_ASSIGN ? null : c);
    // A line's category as chosen. Unassigned is marked (Split.unassigned), so it isn't mistaken
    // for spending that still needs a category; any other choice drops the mark.
    const choose = (old: Partial<Split>, c: string | null): Partial<Split> & Pick<Split, "categoryId"> => {
      const { unassigned: _, ...rest } = old;
      return { ...rest, categoryId: categoryOf(c), ...(!draft && c === READY_TO_ASSIGN ? { unassigned: true } : !c ? { unassigned: false } : {}) };
    };
    if (reverse && initial && transfer === initial.accountId) {
      // Same transfer, edited from the receiving side: keep it recorded from the sending side.
      return {
        ...initial,
        date,
        amount: -amount,
        transferStatus: status,
        transferSource: {
          ...(initial.transferSource ?? {}),
          memo,
          // Only when there is or was a flag, so an unflagged transfer stays as it was.
          ...(flag || initial.transferSource?.flag_color ? { flag_color: flag || null } : {}),
        },
        splits: needsCategory ? [{ ...choose(initial.splits[0] ?? {}, category), amount: -amount }] : [],
      };
    }
    const base: Partial<Transaction> =
      initial && !reverse
        ? initial
        : { id: draftId, approved: initial?.approved ?? true, transferAccountId: null, transferStatus: null };
    const old = (i: number): Partial<Split> => (base.transferAccountId ? {} : (base.splits?.[i] ?? {}));
    const splits: Split[] = transfer
      ? needsCategory
        ? [{ ...choose(base.splits?.[0] ?? {}, category), amount }]
        : []
      : lines
        ? lines.map((l, i) => ({ ...choose(old(i), l.categoryId), memo: l.memo, amount: draft ? safeAmount(l) || 0 : amountOf(l) }))
        : [{ ...(tracking ? { ...old(0), categoryId: null } : choose(old(0), category)), amount }];
    const sameTransfer = !!transfer && base.transferAccountId === transfer;
    return {
      ...base,
      accountId: account,
      date,
      payee: transfer ? "" : payee.trim(),
      memo,
      ...(flag || base.flag ? { flag: flag || null } : {}),
      amount,
      status,
      transferAccountId: transfer,
      transferStatus: transfer ? (sameTransfer ? (base.transferStatus ?? "uncleared") : "uncleared") : null,
      transferSource: transfer ? (sameTransfer && base.transferSource ? base.transferSource : { memo }) : null,
      splits,
    } as Transaction;
  };
  const reset = () => {
    setDraftId(id());
    setPayee("");
    setTransfer(null);
    setCategory(null);
    touchedCategory.current = false;
    setMemo("");
    setFlag("");
    setOutflow("");
    setInflow("");
    setStatus(date > today() ? "uncleared" : newStatus());
    statusSet.current = false;
    setLines(null);
    setRepeat("never");
    setError("");
    focusField("Payee");
  };
  const remaining = (() => {
    if (!lines) return 0;
    const sum = lines.reduce((n, l) => n + safeAmount(l), 0);
    const t = safeAmount({ outflow, inflow });
    return t - sum;
  })();
  const save = async (another = false) => {
    if (busy) return;
    const invalid = row.current?.querySelector<HTMLInputElement>("input:invalid");
    if (invalid) {
      invalid.reportValidity();
      return;
    }
    try {
      const amount = total();
      if (!account) throw new Error(t("Choose an account."));
      if (!date) throw new Error(t("Choose a date."));
      if (!amount) throw new Error(t("Enter an outflow or an inflow."));
      if (transfer && transfer === account) throw new Error(t("Choose a different account to transfer to."));
      if (lines) {
        const parts = lines.map(amountOf);
        const sum = parts.reduce((n, a) => n + a, 0);
        if (sum !== amount) {
          const difference = money(Math.abs(amount - sum));
          throw new Error(
            Math.abs(sum) > Math.abs(amount)
              ? t("The split lines must add up to the total ({amount} over).", { amount: difference })
              : t("The split lines must add up to the total ({amount} left).", { amount: difference }),
          );
        }
        // Spending can be saved with no category, as in YNAB: it says "Needs a category" until
        // it has one (uncategorized.ts).
      }
      setBusy(true);
      setError("");
      const frequency = !initial && repeat !== "never" ? repeat : undefined;
      await (frequency ? onSave(build(amount), frequency) : onSave(build(amount)));
      setBusy(false);
      if (another) reset();
      else onCancel();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };
  const keys = (e: KeyboardEvent) => {
    if (e.defaultPrevented) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void save(!initial && e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };
  const more = () => {
    let amount = 0;
    try {
      amount = total();
    } catch {
      // Unparseable amounts are left for the dialog to ask about.
    }
    onMore(build(amount, true), !initial && repeat !== "never" ? repeat : undefined);
  };
  const split = () => {
    let amount = "";
    try {
      amount = total() ? editableAmount(Math.abs(total())) : "";
    } catch {
      // Keep the lines empty.
    }
    const out = !inflow;
    setLines([
      { categoryId: category, memo: "", outflow: out ? amount : "", inflow: out ? "" : amount },
      { categoryId: null, memo: "", outflow: "", inflow: "" },
    ]);
    requestAnimationFrame(() =>
      row.current?.querySelectorAll<HTMLInputElement>(`.split-line [aria-label^='${t("Category")}']`)[1]?.focus(),
    );
  };
  const setLine = (index: number, patch: Partial<Line>) =>
    setLines((ls) => ls && ls.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  /** A category named in a category field (by its label): create it, choose it, and carry on from that field. */
  const createCategory = (label: string, choose: (id: string) => void) =>
    onCreateCategory &&
    ((name: string) =>
      void onCreateCategory(name).then((created) => {
        if (created) choose(created);
        focusLabel(label);
      }));
  const removeLine = (index: number) => {
    if (!lines) return;
    const next = lines.filter((_, i) => i !== index);
    if (next.length > 1) return setLines(next);
    // One line left: back to a single category.
    setCategory(next[0]?.categoryId ?? null);
    setLines(null);
  };
  /** The outflow and inflow fields, of the transaction or of split `line` (from 1). */
  const amountInputs = (
    value: { outflow: string; inflow: string },
    set: (patch: { outflow?: string; inflow?: string }) => void,
    line?: number,
  ) => (
    <>
      <span role="cell">
        <input
          aria-label={line ? t("Line {number} Outflow", { number: line }) : t("Outflow")}
          inputMode="decimal"
          className="num-input"
          placeholder={t("Outflow")}
          value={value.outflow}
          onChange={(e) => set(e.target.value ? { outflow: e.target.value, inflow: "" } : { outflow: "" })}
        />
      </span>
      <span role="cell">
        <input
          aria-label={line ? t("Line {number} Inflow", { number: line }) : t("Inflow")}
          inputMode="decimal"
          className="num-input"
          placeholder={t("Inflow")}
          value={value.inflow}
          onChange={(e) => set(e.target.value ? { inflow: e.target.value, outflow: "" } : { inflow: "" })}
        />
      </span>
    </>
  );
  return (
    <div
      className="reg-row reg-editor"
      role="row"
      ref={row}
      onKeyDown={keys}
      aria-label={initial ? t("Edit transaction") : t("New transaction")}
    >
      <span role="cell" />
      {/* The flag's menu, drawn as the flag itself while the column is narrow. */}
      <span role="cell" className="flag-cell icon-col">
        <span className={`flag-select${flag ? "" : " empty"}`}>
          <FlagIcon flag={flag || undefined} size={11} outline={!flag} />
          <select aria-label={t("Flag")} value={flag} onChange={(e) => setFlag(e.target.value)}>
            <option value="">{t("No flag")}</option>
            {flagsOf(budget).map(([f, name]) => (
              <option key={f} value={f}>
                {name}
              </option>
            ))}
          </select>
        </span>
      </span>
      <span role="cell" className="date-field">
        <DateInput value={date} onChange={pickDate} />
      </span>
      <span role="cell">
        <PayeeInput
          budget={budget}
          value={payee}
          transfers={transfers}
          onChange={(text) => {
            setPayee(text);
            if (transfer && text !== transferPayee(names.get(transfer) ?? "")) setTransfer(null);
          }}
          onTransfer={(other) => {
            setTransfer(other);
            setLines(null);
          }}
          onPick={(info) => {
            // A payee's usual "none" is Unassigned for income, and nothing chosen for spending.
            if (!touchedCategory.current && info.categoryId !== undefined)
              setCategory(info.categoryId === null ? (income ? READY_TO_ASSIGN : null) : info.categoryId);
          }}
        />
      </span>
      {all && (
        <span role="cell">
          <select aria-label={t("Account")} value={account} onChange={(e) => setAccount(e.target.value)} disabled={reverse}>
            {accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </span>
      )}
      <span role="cell">
        {lines ? (
          <span className="muted">{tn(lines.length, "Split ({count} lines)", "Split ({count} lines)")}</span>
        ) : !needsCategory ? (
          <span className="muted">{transfer ? t("No Category (Transfer)") : t("No Category (Tracking)")}</span>
        ) : (
          <CategoryPicker
            budget={budget}
            value={category}
            onChange={(v) => {
              touchedCategory.current = true;
              setCategory(v);
            }}
            onClear={() => {
              touchedCategory.current = true;
              setCategory(null);
            }}
            onCreate={createCategory(t("Category"), (created) => {
              touchedCategory.current = true;
              setCategory(created);
            })}
            available={available}
            readyToAssign={income ? t(INCOME) : category === READY_TO_ASSIGN ? t(FROM_UNASSIGNED) : false}
            readyToAssignLast={!income}
            includeHidden
            includePayment={payment}
            placeholder={t("Category")}
          />
        )}
      </span>
      <span role="cell" className="memo">
        <input aria-label={t("Memo")} placeholder={t("Memo")} value={memo} onChange={(e) => setMemo(e.target.value)} />
      </span>
      {amountInputs({ outflow, inflow }, (patch) => {
        if (patch.outflow !== undefined) setOutflow(patch.outflow);
        if (patch.inflow !== undefined) setInflow(patch.inflow);
      })}
      {!all && <span role="cell" className="balance" />}
      <span role="cell" className="status-cell icon-col">
        <button
          type="button"
          className={`clear-button ${status}`}
          aria-label={t("Cleared")}
          title={status === "reconciled" ? t("Reconciled") : t("Cleared")}
          aria-pressed={status !== "uncleared"}
          disabled={status === "reconciled"}
          onClick={() => {
            statusSet.current = true;
            setStatus(status === "uncleared" ? "cleared" : "uncleared");
          }}
        >
          <ClearedMark status={status} />
        </button>
      </span>
      {lines?.map((l, i) => (
        <div className="split-line" role="row" aria-label={t("Split line {number}", { number: i + 1 })} key={i}>
          <span role="cell" />
          <span role="cell" className="flag-cell" />
          <span role="cell" />
          <span role="cell" className="split-line-label muted">
            {t("Line {number}", { number: i + 1 })}
          </span>
          {all && <span role="cell" />}
          <span role="cell">
            <CategoryPicker
              budget={budget}
              label={t("Category {number}", { number: i + 1 })}
              value={l.categoryId}
              onChange={(categoryId) => setLine(i, { categoryId })}
              onClear={() => setLine(i, { categoryId: null })}
              onCreate={createCategory(t("Category {number}", { number: i + 1 }), (categoryId) => setLine(i, { categoryId }))}
              available={available}
              readyToAssign={isIncome(l) ? t(INCOME) : l.categoryId === READY_TO_ASSIGN ? t(FROM_UNASSIGNED) : false}
              readyToAssignLast={!isIncome(l)}
              includeHidden
              placeholder={t("Category")}
            />
          </span>
          <span role="cell" className="memo">
            <input
              aria-label={t("Memo {number}", { number: i + 1 })}
              placeholder={t("Memo")}
              value={l.memo}
              onChange={(e) => setLine(i, { memo: e.target.value })}
            />
          </span>
          {amountInputs(l, (patch) => setLine(i, patch), i + 1)}
          {!all && <span role="cell" className="balance" />}
          <span role="cell" className="status-cell icon-col">
            <button type="button" className="icon-button" aria-label={t("Remove line {number}", { number: i + 1 })} onClick={() => removeLine(i)}>
              <X size={14} />
            </button>
          </span>
        </div>
      ))}
      <div className="editor-actions">
        {lines ? (
          <>
            <button
              type="button"
              className="link-button"
              onClick={() => setLines((ls) => ls && [...ls, { categoryId: null, memo: "", outflow: "", inflow: "" }])}
            >
              <Plus size={14} /> {t("Add line")}
            </button>
            <span className={`remaining${remaining ? " warning-text" : ""}`} aria-live="polite">
              {Number.isNaN(remaining) ? t("Check the amounts") : remaining
                ? Math.sign(remaining) === Math.sign(safeAmount({ outflow, inflow }))
                  ? t("{amount} left to split", { amount: money(Math.abs(remaining)) })
                  : t("{amount} over the total", { amount: money(Math.abs(remaining)) })
                : t("Fully split")}
            </span>
          </>
        ) : (
          !transfer && (
            <button type="button" className="link-button" onClick={split} title={t("Divide it between categories")}>
              <SplitIcon size={14} /> {t("Split")}
            </button>
          )
        )}
        {!initial && (
          <label
            className={`repeat-field${repeat === "never" && !ahead ? "" : " on"}`}
            title={ahead ? t("Dated ahead, it's scheduled and entered on that day") : t("Repeat it on a schedule")}
          >
            <Repeat size={14} aria-hidden />
            {/* The choice as text, so the field is as wide as it (the select, laid over it, is as
                wide as its longest option). */}
            <span aria-hidden>{repeat === "never" ? (ahead ? t("Once") : t("Doesn't repeat")) : t(frequencies[repeat])}</span>
            <select aria-label={t("Repeat")} value={repeat} onChange={(e) => setRepeat(e.target.value)}>
              <option value="never">{ahead ? t("Once") : t("Doesn't repeat")}</option>
              {Object.entries(frequencies)
                .filter(([v]) => v !== "never")
                .map(([v, n]) => (
                  <option key={v} value={v}>
                    {t(n)}
                  </option>
                ))}
            </select>
          </label>
        )}
        <button type="button" className="link-button" onClick={more} title={t("The full form, for split transfers and more")}>
          <MoreHorizontal size={14} /> {t("More options")}
        </button>
        {error && (
          <span className="form-error" role="alert">
            {error}
          </span>
        )}
        <span className="spacer" />
        <span className="editor-hint" aria-hidden>
          {t("{enter} saves · {esc} cancels", { enter: <kbd>Enter</kbd>, esc: <kbd>Esc</kbd> })}
        </span>
        <button type="button" className="secondary small" onClick={onCancel}>
          {t("Cancel")}
        </button>
        {!initial && (
          <button
            type="button"
            className="secondary small"
            disabled={busy}
            title="Shift+Enter"
            onClick={() => void save(true)}
          >
            {ahead ? t("Schedule & add another") : t("Save & add another")}
          </button>
        )}
        <button type="button" className="primary small" disabled={busy} onClick={() => void save()}>
          {ahead ? t("Schedule") : t("Save")}
        </button>
      </div>
    </div>
  );
}
