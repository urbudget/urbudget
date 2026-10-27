import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { Lock, MoreHorizontal, Plus, Repeat, Split as SplitIcon, X } from "lucide-react";
import { CategoryPicker, DateInput, PayeeInput, READY_TO_ASSIGN, transferPayee } from "./ui";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import { id, scheduledAhead, today, type Budget, type Split, type Status, type Transaction } from "./types";
import { getPref } from "./prefs";
import { uncategorizedTest } from "./uncategorized";
import { frequencies } from "./Planning";
import { payeeHabits } from "./payees";
import { keyLabel, os } from "./platform";

export const FLAGS: [string, string][] = [
  ["red", "Red"],
  ["orange", "Orange"],
  ["yellow", "Yellow"],
  ["green", "Green"],
  ["blue", "Blue"],
  ["purple", "Purple"],
];
/** A flag's name: what this budget calls it, or its color. */
export const flagName = (flag: string, b?: Pick<Budget, "flagNames"> | null) =>
  b?.flagNames?.[flag]?.trim() || (FLAGS.find(([f]) => f === flag)?.[1] ?? flag);
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
/**
 * "+25" typed into Outflow or "-25" into Inflow, a sign before at most a plain amount: that
 * amount, which moves over to the other field. A longer expression ("-5+10") isn't moved, as it
 * adds up as typed where it is.
 */
const movedBy = (text: string, sign: RegExp) => (sign.test(text) && /^[\d\s.,']*$/.test(text.slice(1)) ? text.slice(1) : null);
/** The picker's Unassigned, as the register says it: income goes to it. Spending isn't offered it,
 * as YNAB steers spending to a category; one already taken from it (a YNAB import, a deleted
 * category) still shows as that until it's given a category. */
export const INCOME = "To Unassigned";
export const FROM_UNASSIGNED = "From Unassigned";
const safeAmount = (l: { outflow: string; inflow: string }) => {
  try {
    return amountOf(l);
  } catch {
    return NaN;
  }
};
/** The fields Tab goes on from by the payee's habits, in order ("Another" and "Save" are the
 * Save & add another and Save buttons). */
const STOPS = ["Payee", "Account", "Category", "Memo", "Outflow", "Inflow", "Cleared", "Another", "Save"];
/** The fields ← and → move between, in the row's order. */
const ACROSS = ["Date", "Payee", "Account", "Category", "Memo", "Outflow", "Inflow", "Cleared"];
/** The C key, whatever the keyboard layout writes on it: its letter, or where a layout has none
 * there (Arabic), the key itself. */
const isC = (e: KeyboardEvent) => e.key.toLowerCase() === "c" || (e.code === "KeyC" && !/^[a-z]$/i.test(e.key));
const mac = os === "macos";
/** Clears the transaction from any field: Alt+C, or ⌃C on a Mac, where ⌥C types ç. */
const clearKey = (e: KeyboardEvent) =>
  isC(e) && !e.metaKey && !e.shiftKey && (mac ? e.ctrlKey && !e.altKey : e.altKey && !e.ctrlKey);
const CLEAR_KEY = mac ? "⌃C" : "Alt+C";
/** Right to left, as the nearest `dir` says (the window's, in Arabic). */
const rtl = (el: Element | null) => el?.closest("[dir]")?.getAttribute("dir") === "rtl";

/**
 * The register's add/edit row. Enter saves, Escape cancels, Tab moves through the fields.
 * Choosing a payee fills in the category last used with it; choosing "Transfer: <account>"
 * makes it a transfer. "Split" divides it into lines that must add up to the total.
 * On a new row, Tab skips the memo and what the payee doesn't usually need (a category it fills
 * in, the other way from its money), and its last amount is suggested; Shift+Tab still steps
 * back through every field, and ← → move to the next field at the end of one's text. Once a
 * transaction is saved with Save & add another, Enter keeps adding them until Escape. Alt+C
 * clears it from any field.
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

  /** A field's control by the name its cell carries (`data-field`, the same in any language):
   * "Another" and "Save" are the Save & add another and Save buttons. */
  const fieldOf = (field: string) =>
    row.current?.querySelector<HTMLInputElement>(
      field === "Another" || field === "Save"
        ? `[data-action="${field}"]`
        : `[data-field="${field}"] input, [data-field="${field}"] select, [data-field="${field}"] button`,
    );
  const focusField = (field: string) => requestAnimationFrame(() => fieldOf(field)?.focus());

  // A new row goes by what the payee's transactions are usually like (payeeHabits).
  const habits = useMemo(
    () => (initial ? null : payeeHabits(budget)),
    [budget.transactions, budget.categories, budget.payees, budget.accounts],
  );
  const known = transfer ? undefined : habits?.get(payee.trim());
  // Until an amount is typed, the payee's last one shows in the field its money usually goes to.
  const [amountTyped, setAmountTyped] = useState(false);
  const suggestion =
    known?.amount && !lines && !amountTyped && !outflow && !inflow
      ? { field: known.income ? "Inflow" : "Outflow", text: editableAmount(Math.abs(known.amount)) }
      : null;
  // Money coming in, or about to: before its amount, a payee whose money usually comes in.
  const incoming = income || (!outflow.trim() && !inflow.trim() && !!known?.income);
  const takeSuggestion = () => {
    if (!suggestion) return;
    setAmountTyped(true);
    if (suggestion.field === "Inflow") setInflow(suggestion.text);
    else setOutflow(suggestion.text);
  };
  /**
   * Where Tab goes from `from` on a new row: the next field this payee is likely to need (Shift+Tab
   * and ← still reach the rest). The category is skipped when the payee filled it in and has had
   * only that one lately (or it's the payee's default), and for money the payee usually pays in
   * with none (it goes to Unassigned); the memo unless the payee usually has one; and the amount
   * field its money doesn't go to. After the cleared mark come Save & add another, then Save.
   */
  const nextStop = (from: string) => {
    const amount = !!(outflow.trim() || inflow.trim());
    const filled = !touchedCategory.current && category !== null && category === known?.category;
    const needed: Record<string, boolean> = {
      Account: true,
      Category: category === null ? !known?.income : !(filled && known?.oneCategory),
      Memo: !memo.trim() && !!known?.memo,
      Outflow: !amount && !known?.income,
      // Tab from an empty Outflow is on its way to Inflow.
      Inflow: !amount && (!!known?.income || from === "Outflow"),
      Cleared: true,
      Another: true,
      Save: true,
    };
    return STOPS.slice(STOPS.indexOf(from) + 1).find((f) => needed[f] && fieldOf(f) && !fieldOf(f)!.disabled);
  };
  // Tab is decided after the row has rendered with the payee field's match (keys).
  const route = useRef(nextStop);
  route.current = nextStop;
  const toggleCleared = () => {
    if (status === "reconciled") return;
    statusSet.current = true;
    setStatus(status === "uncleared" ? "cleared" : "uncleared");
  };
  // The hint says what Space does while the cleared mark has focus.
  const [onMark, setOnMark] = useState(false);
  // How many were saved with Save & add another. Once one is, the row is adding several, and
  // Enter saves and adds another too.
  const [added, setAdded] = useState(0);
  const clearWord = status === "uncleared" ? "clears" : "unclears";
  const enterWord = added ? "saves & adds another" : "saves";
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
    setAmountTyped(false);
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
      if (!account) throw new Error("Choose an account.");
      if (!date) throw new Error("Choose a date.");
      if (!amount) throw new Error("Enter an outflow or an inflow.");
      if (transfer && transfer === account) throw new Error("Choose a different account to transfer to.");
      if (lines) {
        const parts = lines.map(amountOf);
        const sum = parts.reduce((n, a) => n + a, 0);
        if (sum !== amount) throw new Error(`The split lines must add up to the total (${money(Math.abs(amount - sum))} ${Math.abs(sum) > Math.abs(amount) ? "over" : "left"}).`);
        // Spending can be saved with no category, as in YNAB: it says "Needs a category" until
        // it has one (uncategorized.ts).
      }
      setBusy(true);
      setError("");
      const frequency = !initial && repeat !== "never" ? repeat : undefined;
      await (frequency ? onSave(build(amount), frequency) : onSave(build(amount)));
      setBusy(false);
      if (another) {
        setAdded((n) => n + 1);
        reset();
      } else onCancel();
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };
  const keys = (e: KeyboardEvent) => {
    // Keys typed into an input method's composition (Japanese, Chinese…) are its own.
    if (e.defaultPrevented || e.nativeEvent.isComposing || e.keyCode === 229) return;
    const target = e.target as HTMLElement;
    const from = target.closest<HTMLElement>("[data-field]")?.dataset.field ?? "";
    const mark = !!target.closest(".clear-button");
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
    if (e.key === "Enter") {
      // A focused button takes Enter as its click. The cleared mark is part of the transaction,
      // like a field, so Enter there saves it.
      if (target.closest("button") && !mark) return;
      e.preventDefault();
      if (suggestion) {
        // The suggested amount is filled in to be seen before it's saved: the next Enter saves.
        flushSync(takeSuggestion);
        const field = fieldOf(suggestion.field);
        field?.focus();
        field?.select();
        return;
      }
      void save(!initial && (e.shiftKey || added > 0));
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (clearKey(e)) {
      e.preventDefault();
      toggleCleared();
    } else if (mark && plain && isC(e)) {
      e.preventDefault();
      toggleCleared();
    } else if (e.key === "ArrowRight" && plain && !e.shiftKey && suggestion?.field === from) {
      e.preventDefault();
      takeSuggestion();
    } else if (
      (e.key === "ArrowLeft" || e.key === "ArrowRight") &&
      plain &&
      !e.shiftKey &&
      ACROSS.includes(from) &&
      target.getAttribute("aria-expanded") !== "true"
    ) {
      // ← and → go on to the field beside once there's no more text that way, or it's all
      // selected; the field is then selected, as a spreadsheet's cell is. Right to left, the next
      // field is the one on the left, and a field's text ends on whichever side its own runs to.
      const right = e.key === "ArrowRight";
      if (target instanceof HTMLInputElement) {
        const { selectionStart: start, selectionEnd: end, value } = target;
        const all = start === 0 && end === value.length;
        if (!all && (right !== rtl(target) ? end !== value.length : start !== 0)) return;
      }
      const at = ACROSS.indexOf(from);
      const next = (right !== rtl(row.current) ? ACROSS.slice(at + 1) : ACROSS.slice(0, at).reverse())
        .map(fieldOf)
        .find((el) => el && !el.disabled);
      if (!next) return;
      e.preventDefault();
      next.focus();
      if (next.tagName === "INPUT") next.select();
    } else if (e.key === "Tab" && plain && !e.shiftKey && !initial && !lines && STOPS.includes(from)) {
      e.preventDefault();
      // Decided with the payee field's match and any amount taken in the row, and at once, so a
      // key typed straight after goes to the next field. Tab out of the suggested amount takes it.
      flushSync(() => {
        if (suggestion?.field === from) takeSuggestion();
      });
      const to = route.current(from);
      if (to) fieldOf(to)?.focus();
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
      row.current?.querySelectorAll<HTMLInputElement>(".split-line [aria-label^='Category']")[1]?.focus(),
    );
  };
  const setLine = (index: number, patch: Partial<Line>) =>
    setLines((ls) => ls && ls.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  /** A category named in a category field: create it, choose it, and carry on from that field. */
  const createCategory = (field: string, choose: (id: string) => void) =>
    onCreateCategory &&
    ((name: string) =>
      void onCreateCategory(name).then((created) => {
        if (created) choose(created);
        focusField(field as EditorField);
      }));
  const removeLine = (index: number) => {
    if (!lines) return;
    const next = lines.filter((_, i) => i !== index);
    if (next.length > 1) return setLines(next);
    // One line left: back to a single category.
    setCategory(next[0]?.categoryId ?? null);
    setLines(null);
  };
  /** The suggested amount stands in for a field's placeholder. */
  const suggested = (field: string) =>
    suggestion?.field === field
      ? {
          placeholder: suggestion.text,
          className: "num-input suggested",
        }
      : null;
  // Money in typed into Outflow ("+25") goes over to Inflow, and money out typed into Inflow
  // ("-25") to Outflow.
  const amountInputs = (
    value: { outflow: string; inflow: string },
    set: (patch: { outflow?: string; inflow?: string }) => void,
    label = "",
  ) => (
    <>
      <span role="cell" data-field={`${label}Outflow`}>
        <input
          aria-label={`${label}Outflow`}
          inputMode="decimal"
          className="num-input"
          placeholder="Outflow"
          {...(label ? null : suggested("Outflow"))}
          value={value.outflow}
          onChange={(e) => {
            const text = e.target.value;
            const moved = movedBy(text, /^\+/);
            if (moved !== null) {
              set({ inflow: moved, outflow: "" });
              fieldOf(`${label}Inflow`)?.focus();
            } else set(text ? { outflow: text, inflow: "" } : { outflow: "" });
          }}
        />
      </span>
      <span role="cell" data-field={`${label}Inflow`}>
        <input
          aria-label={`${label}Inflow`}
          inputMode="decimal"
          className="num-input"
          placeholder="Inflow"
          {...(label ? null : suggested("Inflow"))}
          value={value.inflow}
          onChange={(e) => {
            const text = e.target.value;
            const moved = movedBy(text, /^[-−]/);
            if (moved !== null) {
              set({ outflow: moved, inflow: "" });
              fieldOf(`${label}Outflow`)?.focus();
            } else set(text ? { inflow: text, outflow: "" } : { inflow: "" });
          }}
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
      aria-label={initial ? "Edit transaction" : "New transaction"}
    >
      <span role="cell" />
      {/* The flag's menu, drawn as the flag itself while the column is narrow. */}
      <span role="cell" className="flag-cell icon-col">
        <span className={`flag-select${flag ? "" : " empty"}`}>
          <FlagIcon flag={flag || undefined} size={11} outline={!flag} />
          <select aria-label="Flag" value={flag} onChange={(e) => setFlag(e.target.value)}>
            <option value="">No flag</option>
            {flagsOf(budget).map(([f, name]) => (
              <option key={f} value={f}>
                {name}
              </option>
            ))}
          </select>
        </span>
      </span>
      {/* Each field's cell names it for Tab and the arrows (fieldOf), whatever its label's language. */}
      <span role="cell" className="date-field" data-field="Date">
        <DateInput value={date} onChange={pickDate} />
      </span>
      <span role="cell" data-field="Payee">
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
            // Its default, else its latest category (payeeHabits). A payee's usual "none" is
            // Unassigned for income (and for a payee whose money comes in, left to it on purpose),
            // and nothing chosen for spending.
            const habit = habits?.get(info.name);
            const usual = habit?.category ?? info.categoryId;
            if (touchedCategory.current || usual === undefined) return;
            const unassigned = usual === READY_TO_ASSIGN && !!habit?.income;
            setCategory(usual !== null && usual !== READY_TO_ASSIGN ? usual : income || unassigned ? READY_TO_ASSIGN : null);
          }}
        />
      </span>
      {all && (
        <span role="cell" data-field="Account">
          <select aria-label="Account" value={account} onChange={(e) => setAccount(e.target.value)} disabled={reverse}>
            {accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </span>
      )}
      <span role="cell" data-field="Category">
        {lines ? (
          <span className="muted">Split ({lines.length} lines)</span>
        ) : !needsCategory ? (
          <span className="muted">{transfer ? "No Category (Transfer)" : "No Category (Tracking)"}</span>
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
            onCreate={createCategory("Category", (created) => {
              touchedCategory.current = true;
              setCategory(created);
            })}
            available={available}
            readyToAssign={incoming ? INCOME : category === READY_TO_ASSIGN ? FROM_UNASSIGNED : false}
            readyToAssignLast={!incoming}
            includeHidden
            includePayment={payment}
            placeholder="Category"
          />
        )}
      </span>
      <span role="cell" className="memo" data-field="Memo">
        <input aria-label="Memo" placeholder="Memo" value={memo} onChange={(e) => setMemo(e.target.value)} />
      </span>
      {amountInputs({ outflow, inflow }, (patch) => {
        setAmountTyped(true);
        if (patch.outflow !== undefined) setOutflow(patch.outflow);
        if (patch.inflow !== undefined) setInflow(patch.inflow);
      })}
      {!all && <span role="cell" className="balance" />}
      <span role="cell" className="status-cell icon-col" data-field="Cleared">
        <button
          type="button"
          className={`clear-button ${status}`}
          aria-label="Cleared"
          title={status === "reconciled" ? "Reconciled" : "Cleared"}
          aria-keyshortcuts={mac ? "Control+C" : "Alt+C"}
          aria-pressed={status !== "uncleared"}
          disabled={status === "reconciled"}
          onClick={toggleCleared}
          onFocus={() => setOnMark(true)}
          onBlur={() => setOnMark(false)}
        >
          <ClearedMark status={status} />
        </button>
      </span>
      {lines?.map((l, i) => (
        <div className="split-line" role="row" aria-label={`Split line ${i + 1}`} key={i}>
          <span role="cell" />
          <span role="cell" className="flag-cell" />
          <span role="cell" />
          <span role="cell" className="split-line-label muted">
            Line {i + 1}
          </span>
          {all && <span role="cell" />}
          <span role="cell" data-field={`Category ${i + 1}`}>
            <CategoryPicker
              budget={budget}
              label={`Category ${i + 1}`}
              value={l.categoryId}
              onChange={(categoryId) => setLine(i, { categoryId })}
              onClear={() => setLine(i, { categoryId: null })}
              onCreate={createCategory(`Category ${i + 1}`, (categoryId) => setLine(i, { categoryId }))}
              available={available}
              readyToAssign={isIncome(l) ? INCOME : l.categoryId === READY_TO_ASSIGN ? FROM_UNASSIGNED : false}
              readyToAssignLast={!isIncome(l)}
              includeHidden
              placeholder="Category"
            />
          </span>
          <span role="cell" className="memo">
            <input
              aria-label={`Memo ${i + 1}`}
              placeholder="Memo"
              value={l.memo}
              onChange={(e) => setLine(i, { memo: e.target.value })}
            />
          </span>
          {amountInputs(l, (patch) => setLine(i, patch), `Line ${i + 1} `)}
          {!all && <span role="cell" className="balance" />}
          <span role="cell" className="status-cell icon-col">
            <button type="button" className="icon-button" aria-label={`Remove line ${i + 1}`} onClick={() => removeLine(i)}>
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
              <Plus size={14} /> Add line
            </button>
            <span className={`remaining${remaining ? " warning-text" : ""}`} aria-live="polite">
              {Number.isNaN(remaining) ? "Check the amounts" : remaining
                ? `${money(Math.abs(remaining))} ${Math.sign(remaining) === Math.sign(safeAmount({ outflow, inflow })) ? "left to split" : "over the total"}`
                : "Fully split"}
            </span>
          </>
        ) : (
          !transfer && (
            <button type="button" className="link-button" onClick={split} title="Divide it between categories">
              <SplitIcon size={14} /> Split
            </button>
          )
        )}
        {!initial && (
          <label
            className={`repeat-field${repeat === "never" && !ahead ? "" : " on"}`}
            title={ahead ? "Dated ahead, it's scheduled and entered on that day" : "Repeat it on a schedule"}
          >
            <Repeat size={14} aria-hidden />
            {/* The choice as text, so the field is as wide as it (the select, laid over it, is as
                wide as its longest option). */}
            <span aria-hidden>{repeat === "never" ? (ahead ? "Once" : "Doesn't repeat") : frequencies[repeat]}</span>
            <select aria-label="Repeat" value={repeat} onChange={(e) => setRepeat(e.target.value)}>
              <option value="never">{ahead ? "Once" : "Doesn't repeat"}</option>
              {Object.entries(frequencies)
                .filter(([v]) => v !== "never")
                .map(([v, n]) => (
                  <option key={v} value={v}>
                    {n}
                  </option>
                ))}
            </select>
          </label>
        )}
        <button type="button" className="link-button" onClick={more} title="The full form, for split transfers and more">
          <MoreHorizontal size={14} /> More options
        </button>
        {error && (
          <span className="form-error" role="alert">
            {error}
          </span>
        )}
        <span className="spacer" />
        <span className="editor-added" aria-live="polite">
          {added ? `${added.toLocaleString()} added` : ""}
        </span>
        {/* Its parts, most useful first (Save & add another shows its own key): short of room, the
            last ones drop off (register.css). */}
        <span className="editor-hint" aria-hidden>
          {onMark ? (
            <>
              <span>
                <kbd>Space</kbd> {clearWord}
              </span>
              <span>
                · <kbd>Enter</kbd> {enterWord}
              </span>
            </>
          ) : (
            <>
              <span>
                <kbd>Enter</kbd> {enterWord}
              </span>
              {status !== "reconciled" && (
                <span>
                  · <kbd>{CLEAR_KEY}</kbd> {clearWord}
                </span>
              )}
              {!initial && !added && (
                <span>
                  · <kbd>{keyLabel("Shift+Enter")}</kbd> adds another
                </span>
              )}
              <span>
                · <kbd>Esc</kbd> {added ? "closes" : "cancels"}
              </span>
            </>
          )}
        </span>
        <button type="button" className="secondary small" onClick={onCancel}>
          {added ? "Close" : "Cancel"}
        </button>
        {/* The blue button is what Enter does: Save & add another once one has been. */}
        {!initial && (
          <button
            type="button"
            className={`${added ? "primary" : "secondary"} small`}
            data-action="Another"
            disabled={busy}
            title={added ? "Enter" : keyLabel("Shift+Enter")}
            aria-keyshortcuts={added ? "Enter" : "Shift+Enter"}
            onClick={() => void save(true)}
          >
            {ahead ? "Schedule & add another" : "Save & add another"}
          </button>
        )}
        <button
          type="button"
          className={`${added ? "secondary" : "primary"} small`}
          data-action="Save"
          disabled={busy}
          aria-keyshortcuts={added ? undefined : "Enter"}
          onClick={() => void save()}
        >
          {ahead ? "Schedule" : "Save"}
        </button>
      </div>
    </div>
  );
}
