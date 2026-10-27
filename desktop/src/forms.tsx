import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type InputHTMLAttributes,
  type ReactNode,
} from "react";
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Plus,
  X,
} from "lucide-react";
import { TargetFields, type TargetMonthContext } from "./TargetFields";
import { frequencies } from "./Planning";
import { flagsOf, FROM_UNASSIGNED, INCOME } from "./TransactionRow";
import { uncategorizedTest } from "./uncategorized";
import { CategoryPicker, DateInput, PayeeInput, READY_TO_ASSIGN } from "./ui";
import { editableAmount, errorText, evaluateAmount } from "./format";
import { getPref } from "./prefs";
import { ACCOUNT_TYPES, accountType, isLiability, isLoanType, kindForType, typeName } from "./accounts";
import { api } from "./bridge";
import { loanTerms } from "./Planning";
import { LoanDetailsFields, loanDraft, loanTermsOf } from "./LoanDetails";
import type { LoanTerms } from "./types";
import {
  id,
  scheduledAhead,
  today,
  type Account,
  type Budget,
  type Category,
  type Transaction,
  type Schedule,
  type Status,
  type Split,
} from "./types";
import { msg, t } from "./i18n";

/** A text field that saves after a pause in typing, on Enter, or on blur — not per keystroke. */
export function CommitInput({
  value,
  onCommit,
  validate,
  ...props
}: {
  value: string;
  onCommit: (value: string) => void;
  validate?: (value: string) => boolean;
} & Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange">) {
  const [text, setText] = useState(value);
  const latest = useRef({ value, onCommit, validate, text });
  latest.current = { value, onCommit, validate, text };
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => setText(value), [value]);
  const commit = () => {
    clearTimeout(timer.current);
    const { value, onCommit, validate, text } = latest.current;
    if (text !== value && (!validate || validate(text))) onCommit(text);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  return (
    <input
      {...props}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        clearTimeout(timer.current);
        timer.current = setTimeout(commit, 800);
      }}
      onBlur={(e) => {
        commit();
        props.onBlur?.(e);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        props.onKeyDown?.(e);
      }}
    />
  );
}

export function Modal({
  title,
  children,
  close,
  wide = false,
  className = "",
}: {
  title: string;
  children: ReactNode;
  close: () => void;
  wide?: boolean;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Capture before React's child autoFocus runs during commit.
  const previousFocus = useRef(document.activeElement as HTMLElement | null);
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    const previous = previousFocus.current;
    const focusable = () => [...(ref.current?.querySelectorAll<HTMLElement>(
      "a[href],button,input,select,textarea,[tabindex],[contenteditable=true]",
    ) ?? [])].filter((el) => {
      if (el.matches(":disabled,input[type=hidden]") || el.tabIndex < 0 || el.closest("[hidden],[inert]")) return false;
      for (let node: HTMLElement | null = el; node; node = node.parentElement) {
        const style = getComputedStyle(node);
        if (style.display === "none" || style.visibility === "hidden") return false;
        if (node === ref.current) break;
      }
      return true;
    });
    const initial = focusable();
    (initial.find((el) => el.hasAttribute("data-autofocus")) ??
      initial.find((el) => el.matches("input,select,textarea")) ?? initial[0])?.focus();
    const key = (e: KeyboardEvent) => {
      // A dialog opened from this one (a new category, say) handles its own keys.
      const open = document.querySelectorAll(".modal");
      if (open[open.length - 1] !== ref.current) return;
      if (e.key === "Escape") closeRef.current();
      if (e.key === "Tab") {
        const nodes = focusable();
        if (!nodes?.length) return;
        if (e.shiftKey && document.activeElement === nodes[0]) {
          e.preventDefault();
          nodes[nodes.length - 1].focus();
        } else if (
          !e.shiftKey &&
          document.activeElement === nodes[nodes.length - 1]
        ) {
          e.preventDefault();
          nodes[0].focus();
        }
      }
    };
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("keydown", key);
      previous?.focus();
    };
  }, []);
  return (
    <div
      className="modal-shade"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        className={`modal${wide ? " wide" : ""}${className ? ` ${className}` : ""}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        ref={ref}
      >
        <div className="modal-heading">
          <h2>{title}</h2>
          <button
            className="icon-button"
            onClick={close}
            aria-label={t("Close dialog")}
          >
            <X size={19} />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function AccountForm({
  initial,
  balance: current,
  budget,
  focusLoan,
  close,
  save,
}: {
  initial?: Account;
  /** The account's balance now, so an untyped tracking account is read as an asset or a debt the
   * way the sidebar and register read it (left out, by its starting balance). */
  balance?: number;
  /** For a debt's loan details (left out, the form has none). */
  budget?: Budget;
  /** Opened from "Edit loan details": start in them. */
  focusLoan?: boolean;
  close: () => void;
  /** `loan` is the loan details to save, null to remove them, or left out when unchanged. */
  save: (a: Account, loan?: LoanTerms | null) => void;
}) {
  const [name, setName] = useState(initial?.name ?? "");
  const [type, setType] = useState(() => {
    if (!initial) return "checking";
    const selected = accountType(initial, current);
    // Imported descriptions can differ from the stored accounting kind (a tracked card).
    // Editing its name must not silently move its history into or out of the budget.
    return kindForType(selected) === initial.kind
      ? selected
      : accountType({ ...initial, accountType: undefined, source: undefined }, current);
  });
  const kind = kindForType(type);
  const [netWorth, setNetWorth] = useState(initial?.netWorth !== false);
  const [balance, setBalance] = useState(
    editableAmount(initial?.openingBalance ?? 0),
  );
  const [accountNote, setAccountNote] = useState(initial?.note ?? "");
  const [archived, setArchived] = useState(initial?.archived ?? false);
  const [error, setError] = useState("");
  const [accountId] = useState(() => initial?.id ?? id());
  const saved = budget && initial ? loanTerms(budget, initial.id) : undefined;
  const [loan, setLoan] = useState(() => loanDraft(saved));
  const [loanChanged, setLoanChanged] = useState(false);
  // A loan imported from YNAB can start from its imported terms, but only when asked: filling
  // them in unasked would save them (and start charging interest) with any edit to the account,
  // and bring them back after they'd been cleared.
  const [importable, setImportable] = useState<LoanTerms | null>(null);
  const [imported, setImported] = useState(false);
  useEffect(() => {
    if (!budget || !initial?.source || saved) return;
    let live = true;
    api
      .sourceLoanTerms(budget, initial.id)
      .then((t) => {
        if (live && t.monthlyPayment > 0) setImportable(t);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);
  // Loan details are for debts outside the budget, and stay editable on any account that has them.
  const showLoan = !!budget && ((kind === "tracking" && isLoanType(type)) || !!saved);
  return (
    <Modal
      title={initial ? t("Edit account") : t("New account")}
      close={close}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          try {
            if (!name.trim()) throw new Error(t("Enter an account name."));
            if (initial && kind !== initial.kind)
              throw new Error(t("Create a new account to change how this account affects your budget."));
            const terms = showLoan && loanChanged ? loanTermsOf(accountId, loan) : undefined;
            const account: Account = {
              ...initial,
              id: accountId,
              name: name.trim(),
              kind,
              accountType: type,
              // Only tracking accounts can stay out of net worth.
              netWorth: kind !== "tracking" || netWorth,
              onBudget: kind !== "tracking" && initial?.onBudget !== false,
              note: accountNote,
              openingBalance: evaluateAmount(balance),
              archived,
            };
            if (terms === undefined) save(account);
            else save(account, terms);
          } catch (e) {
            setError(errorText(e));
          }
        }}
      >
        <label>
          {t("Account name")}
          <input
            required
            maxLength={200}
            placeholder={t("e.g. Everyday account")}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <div className="form-grid">
          <label>
            {t("Account type")}
            <select aria-label={t("Account type")} value={type} onChange={(e) => setType(e.target.value)}>
              {ACCOUNT_TYPES.map((g, i) => (
                <optgroup key={i} label={t(g.group)}>
                  {g.types.map(([value, label]) => (
                    <option key={value} value={value} disabled={!!initial && g.kind !== initial.kind}>
                      {t(label)}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </label>
          <label>
            {t("Opening balance")}
            <input
              required
              inputMode="decimal"
              value={balance}
              onChange={(e) => setBalance(e.target.value)}
            />
          </label>
        </div>
        {initial && (
          <p className="help">
            {t("To switch between cash, credit, and tracking, create a new account so past transactions keep their budget treatment.")}
          </p>
        )}
        <p className="help">
          {kind === "tracking"
            ? `${t("Tracking accounts hold things you own or owe outside the budget; their changes don't affect your categories.")} `
            : `${t("Budget accounts hold the money you give jobs to.")} `}
          {isLiability(type) ? `${t("Enter what you owe as a negative amount.")} ` : ""}
          {t("This balance is the starting point, before any transactions you add.")}
        </p>
        {kind === "tracking" && (
          <label className="checkbox-label">
            <input type="checkbox" checked={netWorth} onChange={(e) => setNetWorth(e.target.checked)} />
            {t("Count this account in net worth")}
          </label>
        )}
        {kind === "tracking" && !netWorth && (
          <p className="help">
            {t("It still shows its balance and appears in reports, but isn't added to net worth, e.g. money a friend owes you that you aren't counting on yet.")}
          </p>
        )}
        {showLoan && budget && (
          <>
            <LoanDetailsFields
              budget={budget}
              type={type}
              value={loan}
              imported={imported}
              onUseImported={
                importable
                  ? () => {
                      setLoan(loanDraft(importable));
                      setLoanChanged(true);
                      setImported(true);
                    }
                  : undefined
              }
              focus={focusLoan}
              onChange={(d) => {
                setLoan(d);
                setLoanChanged(true);
              }}
            />
            {!isLoanType(type) && (
              <p className="help">
                {t("Loan details are for debts. This is set as {type}: choose a loan type above, such as Personal loan, to list it with your debts.", {
                  type: typeName(type).toLowerCase(),
                })}
              </p>
            )}
          </>
        )}
        <label>
          {t("Account notes")}
          <textarea
            value={accountNote}
            onChange={(e) => setAccountNote(e.target.value)}
          />
        </label>
        {initial && (
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={archived}
              onChange={(e) => setArchived(e.target.checked)}
            />
            {t("Archive account (history and balance stay in the budget)")}
          </label>
        )}
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary">
            {initial ? t("Update account") : t("Add account")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function CategoryForm({
  initial,
  groups,
  close,
  save,
  month,
  currentMonth,
}: {
  initial?: Category;
  groups: string[];
  close: () => void;
  save: (c: Category) => void;
} & TargetMonthContext) {
  const [name, setName] = useState(initial?.name ?? "");
  const [group, setGroup] = useState(
    initial?.group ?? groups[0] ?? "Essentials",
  );
  const [categoryNote, setCategoryNote] = useState(initial?.note ?? "");
  const [hidden, setHidden] = useState(initial?.hidden ?? false);
  const [goal, setGoal] = useState<Record<string, unknown>>(
    initial?.goal ?? {
      goal_type: "NEED",
      goal_cadence: 1,
      goal_cadence_frequency: 1,
      goal_needs_whole_amount: true,
      goal_creation_month: `${today().slice(0, 7)}-01`,
    },
  );
  const [goalChanged, setGoalChanged] = useState(false);
  const [target, setTarget] = useState(
    editableAmount(initial?.target ?? 0),
  );
  const [error, setError] = useState("");
  return (
    <Modal
      title={initial ? t("Edit category") : t("New category")}
      close={close}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          try {
            const amount = evaluateAmount(target || "0");
            if (!name.trim() || !group.trim())
              throw new Error(t("Enter a category name and group."));
            if (amount < 0) throw new Error(t("Targets cannot be negative."));
            save({
              ...initial,
              id: initial?.id ?? id(),
              name: name.trim(),
              group: group.trim(),
              target: amount,
              note: categoryNote,
              hidden,
              goal:
                amount === initial?.target && !goalChanged
                  ? initial?.goal
                  : { ...goal, goal_target: amount * 10 },
            });
          } catch (e) {
            setError(String(e));
          }
        }}
      >
        <label>
          {t("Category name")}
          <input
            required
            maxLength={200}
            placeholder={t("e.g. Groceries, travel, a rainy day")}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <div className="form-grid">
          <label>
            {t("Group")}
            <input
              required
              maxLength={200}
              list="groups"
              value={group}
              onChange={(e) => setGroup(e.target.value)}
            />
            <datalist id="groups">
              {groups.map((g) => (
                <option key={g}>{g}</option>
              ))}
            </datalist>
          </label>
          <label>
            {t("Target amount")}
            <input
              inputMode="decimal"
              value={target}
              onChange={(e) => setTarget(e.target.value)}
            />
          </label>
        </div>
        <p className="help">
          {t("A target helps you plan. Set it to zero if you prefer to decide as you go.")}
        </p>
        <TargetFields
          goal={goal}
          month={month}
          currentMonth={currentMonth}
          paymentCategory={!!initial?.paymentAccountId}
          onChange={(g) => {
            setGoal(g);
            setGoalChanged(true);
          }}
        />
        <label>
          {t("Category notes")}
          <textarea
            rows={5}
            value={categoryNote}
            onChange={(e) => setCategoryNote(e.target.value)}
          />
        </label>
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={hidden}
            onChange={(e) => setHidden(e.target.checked)}
          />
          {t("Hidden category")}
        </label>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary">
            {initial ? t("Update category") : t("Add category")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** The transaction form's kinds, as its buttons name them. */
const KIND_NAMES: Record<string, string> = { outflow: msg("Outflow"), inflow: msg("Inflow"), transfer: msg("Transfer") };

export function TransactionForm({
  budget,
  initial,
  defaultAccount,
  close,
  save,
  frequency,
  title,
  saveLabel,
  isNew = !initial,
  repeatable = false,
  initialRepeat = "never",
  schedule = false,
  onCreateCategory,
}: {
  /** Creates a category named in a category field; resolves to its id, or null if cancelled. */
  onCreateCategory?: (name: string) => Promise<string | null>;
  /** A schedule's repeat (ScheduleDialog), "Once" among them, in Repeat's place. */
  frequency?: { value: string; set: (value: string) => void };
  title?: string;
  saveLabel?: string;
  budget: Budget;
  /** The transaction to edit, or the start of a new one (with `isNew`). */
  initial?: Transaction;
  isNew?: boolean;
  /** Offer "Repeat", which also creates a schedule. */
  repeatable?: boolean;
  /** A new draft's repeat choice when continuing from the inline editor. */
  initialRepeat?: string;
  /** A schedule's transaction (ScheduleDialog): it's entered uncleared, so has no cleared boxes. */
  schedule?: boolean;
  defaultAccount?: string;
  close: () => void;
  /** `repeat` is a schedule frequency, when the user chose one. */
  save: (t: Transaction, repeat?: string) => void | Promise<void>;
}) {
  const reverseTransfer =
    !!initial?.transferAccountId &&
    initial.amount > 0 &&
    !initial.splits.length;
  const initialAccount = reverseTransfer ? initial?.transferAccountId : initial?.accountId;
  const initialOther = reverseTransfer ? initial?.accountId : initial?.transferAccountId;
  // A failed save can leave this transaction in the recovery draft. Retrying updates that
  // same transaction instead of adding another one.
  const [transactionId] = useState(() => initial?.id ?? id());
  const [kind, setKind] = useState(
    initial?.transferAccountId
      ? "transfer"
      : (initial?.amount ?? -1) > 0
        ? "inflow"
        : "outflow",
  );
  const [account, setAccount] = useState(
    initialAccount ??
      budget.accounts.find((a) => a.id === defaultAccount && !a.archived)?.id ??
      budget.accounts.find((a) => !a.archived)?.id ??
      "",
  );
  const [other, setOther] = useState(
    initialOther ??
      budget.accounts.find((a) => a.id !== account && !a.archived)?.id ??
      "",
  );
  const [date, setDate] = useState(initial?.date ?? today());
  const [repeat, setRepeat] = useState(initialRepeat);
  const [payee, setPayee] = useState(initial?.payee ?? "");
  const [memo, setMemo] = useState(
    String((reverseTransfer ? initial?.transferSource?.memo ?? initial?.memo : initial?.memo) ?? ""),
  );
  const [flag, setFlag] = useState(
    String((reverseTransfer ? initial?.transferSource?.flag_color : initial?.flag) ?? ""),
  );
  const [transferFlag, setTransferFlag] = useState(
    String((reverseTransfer ? initial?.flag : initial?.transferSource?.flag_color) ?? ""),
  );
  const [transferMemo, setTransferMemo] = useState(
    String((reverseTransfer ? initial?.memo : initial?.transferSource?.memo ?? initial?.memo) ?? ""),
  );
  const [amount, setAmount] = useState(
    initial?.amount ? editableAmount(Math.abs(initial.amount)) : "",
  );
  const sourceStatus = reverseTransfer
    ? initial?.transferStatus
    : initial?.status;
  const destinationStatus = reverseTransfer
    ? initial?.status
    : initial?.transferStatus;
  const [status, setStatus] = useState<Status>(
    sourceStatus ?? (isNew && getPref("newCleared") ? "cleared" : "uncleared"),
  );
  const [transferStatus, setTransferStatus] = useState<Status>(
    destinationStatus ?? "uncleared",
  );
  // Dated ahead and not yet cleared, it's a scheduled transaction, entered on its date
  // (scheduledAhead, saveTransaction).
  const ahead = date > today() && status === "uncleared" && (kind !== "transfer" || transferStatus === "uncleared") &&
    (!initial || scheduledAhead({ ...initial, date, status, transferStatus: kind === "transfer" ? transferStatus : null }));
  // Nothing dated ahead has cleared yet: a new one dated ahead starts uncleared, whatever
  // Settings says, until a cleared box is set by hand.
  const statusSet = useRef(false);
  const pickDate = (next: string) => {
    setDate(next);
    if (!isNew || statusSet.current) return;
    setStatus(next > today() || !getPref("newCleared") ? "uncleared" : "cleared");
    setTransferStatus("uncleared");
  };
  const [splits, setSplits] = useState<
    (Omit<Split, "amount" | "categoryId" | "memo" | "payee"> & {
      categoryId: string;
      amount: string;
      memo: string;
      payee: string;
    })[]
  >(
    initial?.splits.length
      ? initial.splits.map((s) => ({
          ...s,
          memo: s.memo ?? "",
          payee: s.payee ?? "",
          // No category means none chosen yet in a new transaction's draft, and in spending that
          // still needs one (whatever its date); otherwise it was Unassigned.
          categoryId: s.categoryId ?? (isNew || uncategorizedTest(budget, "9999-12-31")(initial, s) ? "" : READY_TO_ASSIGN),
          amount: editableAmount(s.amount * (initial.amount < 0 ? -1 : 1)),
        }))
      : [{ categoryId: "", amount: "", memo: "", payee: "" }],
  );
  const needsCategory =
    kind !== "transfer" ||
    (budget.accounts.find((a) => a.id === account)?.kind === "tracking") !==
      (budget.accounts.find((a) => a.id === other)?.kind === "tracking");
  const tracking =
    budget.accounts.find((a) => a.id === account)?.kind === "tracking";
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const submit = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (saving.current || !e.currentTarget.reportValidity()) return;
    try {
      const total = evaluateAmount(amount);
      if (total < 0)
        throw new Error(t("Enter a non-negative transaction amount."));
      const sign =
        kind === "inflow" ||
        (kind === "transfer" &&
          initial &&
          initial.amount > 0 &&
          initial.splits.length)
          ? 1
          : -1;
      if (kind === "transfer" && (!other || other === account))
        throw new Error(t("Choose a different account for this transfer."));
      // Spending can be saved with no category, as in YNAB: it says "Needs a category" until it
      // has one (uncategorized.ts).
      const parsed = !needsCategory
        ? []
        : splits.map(({ unassigned: _, ...s }) => ({
            ...s,
            // Unassigned chosen is marked, so it isn't mistaken for none chosen (uncategorized.ts).
            ...(s.categoryId === READY_TO_ASSIGN ? { unassigned: true } : !s.categoryId ? { unassigned: false } : {}),
            categoryId:
              s.transferAccountId &&
              (budget.accounts.find((a) => a.id === s.transferAccountId)
                ?.kind !==
                "tracking") ===
                !tracking
                ? null
                : s.categoryId === "__rta"
                  ? null
                  : s.categoryId || null,
            ...(s.transferAccountId
              ? {
                  transferAccountId:
                    kind === "transfer" ? null : s.transferAccountId,
                  transferStatus: s.transferStatus ?? "uncleared",
                }
              : {}),
            amount: (splits.length === 1 ? total : evaluateAmount(s.amount)) * sign,
          }));
      if (
        needsCategory &&
        parsed.reduce((n, s) => n + s.amount, 0) !== total * sign
      )
        throw new Error(t("The split amounts must equal the transaction total."));
      const frequency = repeatable && repeat !== "never" ? repeat : undefined;
      const transaction: Transaction = {
        ...initial,
        id: transactionId,
        date,
        accountId: account,
        payee: kind === "transfer" ? "" : payee.trim(),
        memo,
        flag: flag || null,
        transferSource:
          kind === "transfer"
            ? {
                ...initial?.transferSource,
                memo: transferMemo,
                flag_color: transferFlag || null,
              }
            : null,
        amount: total * sign,
        status,
        splits: parsed,
        transferAccountId: kind === "transfer" ? other : null,
        transferStatus: kind === "transfer" ? transferStatus : null,
      };
      saving.current = true;
      setBusy(true);
      setError("");
      await (frequency ? save(transaction, frequency) : save(transaction));
    } catch (e) {
      setError(errorText(e));
    } finally {
      saving.current = false;
      setBusy(false);
    }
  };
  const kinds = ["outflow", "inflow", "transfer"];
  const kindName = (k: string) => t(KIND_NAMES[k] ?? k);
  const kindIcon = (k: string, size = 15) =>
    k === "outflow" ? <ArrowUpRight size={size} /> : k === "inflow" ? <ArrowDownLeft size={size} /> : <ArrowLeftRight size={size} />;
  const pickKind = (k: string) => {
    setKind(k);
    setError("");
  };
  const accountField = (
    <label>
      <span>
        {kind === "transfer"
          ? initial && initial.amount > 0 && initial.splits.length
            ? t("To account")
            : t("From account")
          : t("Account")}
      </span>
      <select value={account} onChange={(e) => setAccount(e.target.value)}>
        {budget.accounts
          .filter((a) => !a.archived || a.id === initialAccount)
          .map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
      </select>
    </label>
  );
  const dateField = (
    <label>
      <span>{t("Date")}</span>
      <DateInput required value={date} onChange={pickDate} />
    </label>
  );
  const repeatField = frequency ? (
    <label>
      <span>{t("Repeat")}</span>
      <select aria-label={t("Repeat frequency")} value={frequency.value} onChange={(e) => frequency.set(e.target.value)}>
        {Object.entries(frequencies).map(([v, n]) => (
          <option key={v} value={v}>
            {t(n)}
          </option>
        ))}
      </select>
    </label>
  ) : (
    repeatable && (
      <label>
        <span>{t("Repeat")}</span>
        <select aria-label={t("Repeat")} value={repeat} onChange={(e) => setRepeat(e.target.value)}>
          <option value="never">{ahead ? t("Once") : t("Never")}</option>
          {Object.entries(frequencies)
            .filter(([v]) => v !== "never")
            .map(([v, n]) => (
              <option key={v} value={v}>
                {t(n)}
              </option>
            ))}
        </select>
      </label>
    )
  );
  const whoField =
    kind === "transfer" ? (
      <label>
        <span>{initial && initial.amount > 0 && initial.splits.length ? t("From account") : t("To account")}</span>
        <select value={other} onChange={(e) => setOther(e.target.value)}>
          <option value="">{t("Choose account")}</option>
          {budget.accounts
            .filter((a) => a.id !== account && (!a.archived || a.id === initialOther))
            .map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
        </select>
      </label>
    ) : (
      <div className="field">
        <span>{t("Payee")}</span>
        <PayeeInput
          budget={budget}
          value={payee}
          onChange={setPayee}
          placeholder={t("Who was it for?")}
          onPick={(info) => {
            // Fill in the payee's usual category when none is chosen yet. Its usual "none" is
            // Unassigned only for income; spending is left for a category.
            if (info.categoryId === undefined || splits.length !== 1 || splits[0].categoryId) return;
            if (info.categoryId === null && kind !== "inflow") return;
            setSplits((rows) => [{ ...rows[0], categoryId: info.categoryId ?? READY_TO_ASSIGN }]);
          }}
        />
      </div>
    );
  const amountField = (
    <label>
      {t("Amount ({currency})", { currency: budget.currency })}
      <input required inputMode="decimal" placeholder="0.00" value={amount} onChange={(e) => setAmount(e.target.value)} />
    </label>
  );
  const categoryField = needsCategory && (
    <div className="split-section">
      <div className="split-label">
        {splits.length > 1 ? t("SPLIT CATEGORIES") : t("CATEGORY")}
        <button
          type="button"
          className="text-button"
          onClick={() => setSplits((s) => [...s, { categoryId: "", amount: "", memo: "", payee: "" }])}
        >
          <Plus size={14} />
          {t("Split transaction")}
        </button>
      </div>
      {splits.map((s, index) => (
        <div className="split-row" key={index}>
          <CategoryPicker
            budget={budget}
            label={t("Category {number}", { number: index + 1 })}
            value={s.categoryId || null}
            onChange={(categoryId) => setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, categoryId } : r)))}
            onClear={() => setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, categoryId: "" } : r)))}
            onCreate={
              onCreateCategory &&
              ((name) =>
                void onCreateCategory(name).then((categoryId) => {
                  if (categoryId) setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, categoryId } : r)));
                }))
            }
            readyToAssign={kind === "inflow" ? t(INCOME) : s.categoryId === READY_TO_ASSIGN ? t(FROM_UNASSIGNED) : false}
            readyToAssignLast={kind !== "inflow"}
            includeHidden
            includePayment={!!budget.categories.find((c) => c.id === s.categoryId)?.paymentAccountId}
            placeholder={kind === "inflow" ? t(INCOME) : t("Choose a category")}
          />
          {splits.length > 1 && kind !== "transfer" && (
            <>
              <select
                aria-label={t("Transfer account {number}", { number: index + 1 })}
                value={s.transferAccountId ?? ""}
                onChange={(e) =>
                  setSplits((rows) =>
                    rows.map((r, i) =>
                      i === index
                        ? { ...r, transferAccountId: e.target.value || null, transferStatus: r.transferStatus ?? "uncleared" }
                        : r,
                    ),
                  )
                }
              >
                <option value="">{t("No transfer")}</option>
                {budget.accounts
                  .filter((a) => a.id !== account && (!a.archived || a.id === s.transferAccountId))
                  .map((a) => (
                    <option key={a.id} value={a.id}>
                      {t("Transfer: {account}", { account: a.name })}
                    </option>
                  ))}
              </select>
              {s.transferAccountId && (
                <select
                  aria-label={t("Transfer status {number}", { number: index + 1 })}
                  value={s.transferStatus ?? "uncleared"}
                  onChange={(e) =>
                    setSplits((rows) =>
                      rows.map((r, i) => (i === index ? { ...r, transferStatus: e.target.value as Status } : r)),
                    )
                  }
                >
                  <option value="uncleared">{t("Uncleared")}</option>
                  <option value="cleared">{t("Cleared")}</option>
                  <option value="reconciled">{t("Reconciled")}</option>
                </select>
              )}
            </>
          )}
          {splits.length > 1 && (
            <>
              <input
                aria-label={t("Split amount {number}", { number: index + 1 })}
                inputMode="decimal"
                placeholder="0.00"
                value={s.amount}
                onChange={(e) => setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, amount: e.target.value } : r)))}
              />
              <input
                aria-label={t("Split memo {number}", { number: index + 1 })}
                placeholder={t("Split memo")}
                value={s.memo}
                onChange={(e) => setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, memo: e.target.value } : r)))}
              />
              <input
                aria-label={t("Split payee {number}", { number: index + 1 })}
                placeholder={t("Split payee")}
                value={s.payee}
                onChange={(e) => setSplits((rows) => rows.map((r, i) => (i === index ? { ...r, payee: e.target.value } : r)))}
              />
              <button
                type="button"
                className="icon-button"
                aria-label={t("Remove split {number}", { number: index + 1 })}
                onClick={() => setSplits((rows) => rows.filter((_, i) => i !== index))}
              >
                <X size={15} />
              </button>
            </>
          )}
        </div>
      ))}
      {splits.length > 1 && (
        <p className="help">{t("Split amounts must add up to the total above. Use a negative split for a refund within a purchase.")}</p>
      )}
    </div>
  );
  const flags = flagsOf(budget);
  const flagField = (
    <label>
      <span>{t("Flag")}</span>
      <select value={flag} onChange={(e) => setFlag(e.target.value)}>
        <option value="">{t("None")}</option>
        {flags.map(([f, name]) => (
          <option key={f} value={f}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
  const transferFlagField = kind === "transfer" && (
    <label>
      <span>{t("Other account’s flag")}</span>
      <select value={transferFlag} onChange={(e) => setTransferFlag(e.target.value)}>
        <option value="">{t("None")}</option>
        {flags.map(([f, name]) => (
          <option key={f} value={f}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
  const transferMemoField = kind === "transfer" && (
    <label>
      <span>{t("Other account’s memo")}</span>
      <textarea value={transferMemo} onChange={(e) => setTransferMemo(e.target.value)} />
    </label>
  );
  const memoField = (
    <label>
      <span>
        {t("Memo")}<span className="optional">{t("optional")}</span>
      </span>
      <textarea
        rows={2}
        maxLength={5000}
        placeholder={t("Anything you want to remember")}
        value={memo}
        onChange={(e) => setMemo(e.target.value)}
      />
    </label>
  );
  const clearedFields = !schedule && (
    <div className="button-row">
      <label className="checkbox-label">
        <input
          type="checkbox"
          checked={status !== "uncleared"}
          onChange={(e) => {
            statusSet.current = true;
            setStatus(e.target.checked ? "cleared" : "uncleared");
          }}
        />
        {kind === "transfer" ? t("Cleared in source account") : t("Cleared at the bank")}
      </label>
      {kind === "transfer" && (
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={transferStatus !== "uncleared"}
            onChange={(e) => {
              statusSet.current = true;
              setTransferStatus(e.target.checked ? "cleared" : "uncleared");
            }}
          />
          {t("Cleared in destination")}
        </label>
      )}
    </div>
  );
  const notes = (
    <>
      {(initial?.status === "reconciled" || initial?.transferStatus === "reconciled") && (
        <p className="help warning-text">
          {t("This transaction is reconciled. Changing its amount, date, or account changes previously reconciled history.")}
        </p>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </>
  );
  const actions = (
    <div className="modal-actions">
      <button type="button" className="secondary" onClick={close}>
        {t("Cancel")}
      </button>
      <button className="primary" disabled={busy}>
        {busy ? t("Saving…") : saveLabel ?? (ahead ? t("Schedule") : isNew ? t("Add transaction") : t("Update transaction"))}
      </button>
    </div>
  );
  const heading = title ?? (isNew ? t("New transaction") : t("Edit transaction"));
  return (
    <Modal title={heading} close={close}>
      <form onSubmit={submit}>
        <div className="segmented">
          {kinds.map((k) => (
            <button type="button" key={k} className={kind === k ? "selected" : ""} onClick={() => pickKind(k)}>
              {kindIcon(k)} {kindName(k)}
            </button>
          ))}
        </div>
        <div className="form-grid tx-where">
          {accountField}
          {dateField}
          {repeatField}
        </div>
        <div className="form-grid tx-who">
          {whoField}
          {amountField}
        </div>
        {categoryField}
        {flagField}
        {transferFlagField}
        {transferMemoField}
        {memoField}
        {clearedFields}
        {notes}
        {actions}
      </form>
    </Modal>
  );
}

/**
 * Edits a scheduled transaction, or makes a new one (`schedule` left out): its transaction, its
 * next date and how often it repeats ("Once" for a one-time one). `save` stores it; one dated
 * today or earlier is entered then (saveSchedule).
 */
export function ScheduleDialog({
  budget,
  schedule,
  defaultAccount,
  save,
  close,
  onCreateCategory,
}: {
  budget: Budget;
  schedule?: Schedule;
  /** A new one's account: the register it was made from. */
  defaultAccount?: string;
  save: (s: Schedule) => void | Promise<void>;
  close: () => void;
  onCreateCategory?: (name: string) => Promise<string | null>;
}) {
  const [frequency, setFrequency] = useState(schedule?.frequency ?? "monthly");
  // A new one starts tomorrow, the soonest it can wait for.
  const [draft] = useState<Transaction>(
    () =>
      schedule?.transaction ?? {
        id: id(),
        date: "",
        accountId:
          budget.accounts.find((a) => a.id === defaultAccount && !a.archived)?.id ??
          budget.accounts.find((a) => !a.archived)?.id ?? "",
        payee: "",
        memo: "",
        amount: 0,
        status: "uncleared",
        splits: [],
        transferAccountId: null,
        transferStatus: null,
      },
  );
  const tomorrow = (() => {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    return d.toLocaleDateString("en-CA");
  })();
  return (
    <TransactionForm
      budget={budget}
      initial={{ ...draft, date: schedule?.nextDate ?? tomorrow }}
      isNew={!schedule}
      defaultAccount={defaultAccount}
      close={close}
      onCreateCategory={onCreateCategory}
      title={schedule ? t("Scheduled transaction") : t("New scheduled transaction")}
      schedule
      saveLabel={t("Save schedule")}
      frequency={{ value: frequency, set: setFrequency }}
      save={async (tx) => {
        await save({
          paused: false,
          ...schedule,
          id: schedule?.id ?? tx.id,
          transaction: tx,
          nextDate: tx.date,
          // A new next date starts the pattern again from there.
          anchorDate: schedule && schedule.nextDate === tx.date ? schedule.anchorDate : tx.date,
          frequency,
        });
        close();
      }}
    />
  );
}

export function ReconcileForm({
  account,
  cleared,
  money,
  close,
  save,
}: {
  account: Account;
  cleared: number;
  money: (n: number) => string;
  close: () => void;
  save: (balance: number) => Promise<void>;
}) {
  const [balance, setBalance] = useState(editableAmount(cleared));
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  let difference = 0;
  try {
    difference = evaluateAmount(balance) - cleared;
  } catch {
    /* validated on submit */
  }
  return (
    <Modal title={t("Reconcile {account}", { account: account.name })} close={close}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          try {
            await save(evaluateAmount(balance));
          } catch (e) {
            setError(String(e));
            setBusy(false);
          }
        }}
      >
        <p>
          {t("Your cleared balance is {balance}. Compare it with the bank’s posted balance, excluding pending transactions.", {
            balance: <strong>{money(cleared)}</strong>,
          })}
        </p>
        <label>
          {t("Bank balance")}
          <input
            required
            inputMode="decimal"
            value={balance}
            onChange={(e) => setBalance(e.target.value)}
          />
        </label>
        {difference !== 0 ? (
          <div className="tip warning">
            {t("Difference: {amount}. Continuing creates a clearly labeled reconciliation adjustment in Unassigned. Review missing transactions first if this difference is unexpected.", {
              amount: <strong>{money(difference)}</strong>,
            })}
          </div>
        ) : (
          <div className="tip">{t("Balances match. All cleared entries will be marked reconciled.")}</div>
        )}
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary" disabled={busy}>
            {busy
              ? t("Reconciling…")
              : difference
                ? t("Add adjustment & reconcile")
                : t("Finish reconciliation")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
