// The setup walkthrough (docs/plans/first-budget-walkthrough.md): five short screens, each one
// question and one filled button, that end with a funded budget. It runs two ways with the same
// screens. Before the budget exists the answers are a draft kept on the device, and the engine
// builds the budget from them when they're skipped or assigned (setup.rs). On a budget that
// exists (the last step, or one reopened from the setup card) each step is a normal edit.
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Plus, X } from "lucide-react";
import { accountType, typeName } from "./accounts";
import { api } from "./bridge";
import { cloud } from "./cloud";
import { groupNames, orderedCategories } from "./categories";
import { errorText, money, numberFormatFor, type NumberFormat } from "./format";
import { t, tn, useLanguage } from "./i18n";
import { autoAssignAction } from "./QuickAssign";
import {
  addAccounts,
  addCategories,
  answersOf,
  blankRow,
  clearDraft,
  draftCategories,
  leftoverCategory,
  looseCents,
  monthly,
  newDraft,
  ownCategories,
  readDraft,
  readMark,
  SETUP_ACCOUNT_TYPES,
  stepsLeft,
  typedCents,
  updateMark,
  writeDraft,
  writeMark,
  MARK_EVENT,
  type AccountRow,
  type AmountEntry,
  type KeepRoute,
  type SetupCategory,
  type SetupDraft,
  type SetupOrigin,
  type SetupPreview,
} from "./setup";
import { TemplateSelect, useBudgetTemplates } from "./TemplatePicker";
import { useToast } from "./ui";
import type { Budget, BudgetTemplate, Summary } from "./types";
import "./setup.css";

/** A walkthrough in progress: on a draft kept for the welcome screen or the dialog, or on the open budget. */
export type SetupSession = { mode: "draft"; origin: SetupOrigin } | { mode: "budget"; step: number };

type Money = { currency: string; numberFormat: NumberFormat };
const show = (cents: number, format: Money) => money(cents, { ...format, symbol: true });
const editable = (cents: number, format: Money) => (cents / 100).toFixed(2).replace(".", format.numberFormat.decimal);

// ---------- The frame every step sits in ----------

function Frame({
  step,
  title,
  lead,
  children,
  error,
  busy,
  back,
  skip,
  action,
  label,
  blocked = false,
}: {
  step: number;
  title: string;
  lead?: ReactNode;
  children: ReactNode;
  error?: string;
  busy: boolean;
  back?: () => void;
  skip: () => void;
  action: () => void;
  /** The one filled button. */
  label: string;
  blocked?: boolean;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  // A new step starts at its top, with the question read out.
  useEffect(() => {
    heading.current?.focus();
    heading.current?.scrollIntoView?.({ block: "start" });
  }, [step]);
  return (
    <form
      className="setup"
      aria-labelledby="setup-title"
      onSubmit={(e) => {
        e.preventDefault();
        if (!busy && !blocked) action();
      }}
    >
      <p className="setup-count">{t("Step {step} of {total}", { step, total: 5 })}</p>
      <h1 id="setup-title" ref={heading} tabIndex={-1}>
        {title}
      </h1>
      {lead && <p className="setup-lead">{lead}</p>}
      <div className="setup-body">{children}</div>
      <div className="setup-actions">
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        {back && (
          <button type="button" className="secondary" disabled={busy} onClick={back}>
            {t("Back")}
          </button>
        )}
        <span className="setup-spacer" />
        <button type="button" className="link-button" disabled={busy} onClick={skip}>
          {t("Skip setup")}
        </button>
        <button className="primary" disabled={busy || blocked}>
          {label}
        </button>
      </div>
    </form>
  );
}

// ---------- Step 1: accounts ----------

type FixedAccount = { id: string; name: string; type: string; balance: number };

function AccountRows({
  fixed = [],
  rows,
  onRows,
  format,
}: {
  /** Accounts the budget already has. */
  fixed?: FixedAccount[];
  rows: AccountRow[];
  onRows: (rows: AccountRow[]) => void;
  format: Money;
}) {
  const decimal = format.numberFormat.decimal;
  const change = (i: number, patch: Partial<AccountRow>) => onRows(rows.map((row, at) => (at === i ? { ...row, ...patch } : row)));
  // The money to plan with is what the cash accounts hold: a card is owed, not had.
  const cash =
    fixed.filter((a) => a.type !== "creditCard" && a.type !== "lineOfCredit").reduce((sum, a) => sum + a.balance, 0) +
    rows.filter((row) => row.type !== "creditCard" && row.name.trim()).reduce((sum, row) => sum + looseCents(row.balance, decimal), 0);
  return (
    <>
      {fixed.length > 0 && (
        <ul className="setup-fixed">
          {fixed.map((a) => (
            <li key={a.id}>
              <strong>{a.name}</strong>
              <span>{typeName(a.type)}</span>
              <span className="setup-figure">{show(a.balance, format)}</span>
            </li>
          ))}
        </ul>
      )}
      {rows.map((row, i) => (
        <div className="setup-row" role="group" aria-label={t("Account {number}", { number: fixed.length + i + 1 })} key={i}>
          <label>
            {t("Name")}
            <input
              autoFocus={i === 0 && !fixed.length && !row.name}
              maxLength={200}
              placeholder={t("e.g. Main account")}
              value={row.name}
              onChange={(e) => change(i, { name: e.target.value })}
            />
          </label>
          <label>
            {t("Type")}
            <select value={row.type} onChange={(e) => change(i, { type: e.target.value as AccountRow["type"] })}>
              {SETUP_ACCOUNT_TYPES.map((type) => (
                <option key={type} value={type}>
                  {typeName(type)}
                </option>
              ))}
            </select>
          </label>
          <label>
            {row.type === "creditCard" ? t("What you owe") : t("Today's balance")}
            <input inputMode="decimal" dir="ltr" placeholder={editable(0, format)} value={row.balance} onChange={(e) => change(i, { balance: e.target.value })} />
          </label>
          <button
            type="button"
            className="icon-button"
            title={t("Remove this account")}
            aria-label={t("Remove this account")}
            disabled={rows.length === 1 && !fixed.length && !row.name && !row.balance}
            onClick={() => onRows(rows.length === 1 && !fixed.length ? [blankRow()] : rows.filter((_, at) => at !== i))}
          >
            <X size={15} />
          </button>
        </div>
      ))}
      <p>
        <button type="button" className="link-button" onClick={() => onRows([...rows, blankRow()])}>
          <Plus size={14} /> {t("Add another")}
        </button>
      </p>
      <p className="setup-sum">{t("You have {amount} to plan with.", { amount: <strong>{show(cash, format)}</strong> })}</p>
    </>
  );
}

/** Why the typed accounts can't go on, or "" when they can. */
function accountsProblem(rows: AccountRow[], existing: number, decimal: NumberFormat["decimal"]): string {
  try {
    for (const row of rows) {
      const balance = typedCents(row.balance, decimal);
      if (!row.name.trim() && balance !== null) return t("Give the account a name, or clear its balance.");
    }
  } catch (e) {
    return errorText(e);
  }
  return existing + rows.filter((row) => row.name.trim()).length ? "" : t("Add one account with a name to go on.");
}

// ---------- Step 2: categories ----------

/** A group's categories as chips. A template's can be left out and put back; an added one is taken away. */
function Chips({ name, children }: { name: string; children: ReactNode }) {
  return (
    <section className="setup-group">
      <h2>{name}</h2>
      <div className="setup-chips">{children}</div>
    </section>
  );
}

function CategoryChoice({
  existing,
  templates,
  templateId,
  onTemplate,
  leftOut,
  onLeftOut,
  added,
  onAdded,
}: {
  /** The budget's own categories by group, when it has some: they stay, and more can be added. */
  existing?: { name: string; categories: string[] }[];
  templates: BudgetTemplate[] | null;
  templateId: string;
  onTemplate: (id: string) => void;
  leftOut: string[];
  onLeftOut: (keys: string[]) => void;
  added: string[];
  onAdded: (names: string[]) => void;
}) {
  const [adding, setAdding] = useState<string | null>(null);
  const template = templates?.find((x) => x.id === templateId);
  const add = () => {
    const name = adding?.trim();
    if (name) onAdded([...added, name]);
    setAdding(name ? "" : null);
  };
  const own = added.map((name, n) => ({ name, n })).filter((x) => x.name.trim());
  return (
    <>
      {existing ? (
        existing.map((group) => (
          <Chips key={group.name} name={group.name}>
            {group.categories.map((name, i) => (
              <span key={i} className="setup-chip fixed">
                {name}
              </span>
            ))}
          </Chips>
        ))
      ) : (
        <>
          <TemplateSelect value={templateId} onChange={onTemplate} templates={templates} />
          {template?.groups.map((group, g) => (
            <Chips key={group.name} name={group.name}>
              {group.categories.map((name, c) => {
                const key = `${g}.${c}`;
                const out = leftOut.includes(key);
                return (
                  <button
                    key={key}
                    type="button"
                    className={`setup-chip${out ? " out" : ""}`}
                    aria-pressed={!out}
                    title={out ? t("Left out. Click to put it back.") : t("Click to leave it out.")}
                    onClick={() => onLeftOut(out ? leftOut.filter((x) => x !== key) : [...leftOut, key])}
                  >
                    {name}
                  </button>
                );
              })}
            </Chips>
          ))}
        </>
      )}
      {own.length > 0 && (
        <Chips name={t("Other")}>
          {own.map(({ name, n }) => (
            <span key={n} className="setup-chip own">
              {name}
              <button
                type="button"
                className="setup-chip-remove"
                aria-label={t("Remove {name}", { name })}
                title={t("Remove {name}", { name })}
                // Blanked, not dropped, so the others keep the place their amounts are kept under.
                onClick={() => onAdded(added.map((x, at) => (at === n ? "" : x)))}
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </Chips>
      )}
      {adding === null ? (
        <p>
          <button type="button" className="link-button" onClick={() => setAdding("")}>
            <Plus size={14} /> {t("Add your own")}
          </button>
        </p>
      ) : (
        <div className="setup-add">
          <input
            autoFocus
            aria-label={t("Category name")}
            placeholder={t("Category name")}
            maxLength={200}
            value={adding}
            onChange={(e) => setAdding(e.target.value)}
            onKeyDown={(e) => {
              // Enter adds the category; it doesn't press Next.
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              } else if (e.key === "Escape") setAdding(null);
            }}
          />
          <button type="button" className="secondary" onClick={add}>
            {t("Add")}
          </button>
        </div>
      )}
    </>
  );
}

// ---------- Step 3: what a month costs ----------

function AmountRows({
  categories,
  amounts,
  onAmounts,
  income,
  onIncome,
  have,
  format,
}: {
  categories: SetupCategory[];
  amounts: Record<string, AmountEntry>;
  onAmounts: (amounts: Record<string, AmountEntry>) => void;
  income: string;
  onIncome: (text: string) => void;
  /** The money there is to plan with. */
  have: number;
  format: Money;
}) {
  const decimal = format.numberFormat.decimal;
  const groups = [...new Set(categories.map((c) => c.group))];
  const entry = (key: string): AmountEntry => amounts[key] ?? { amount: "" };
  const cost = categories.reduce((sum, c) => sum + monthly(Math.max(0, looseCents(entry(c.key).amount, decimal)), entry(c.key).yearly), 0);
  return (
    <>
      {groups.map((group) => (
        <section className="setup-group" key={group}>
          <h2>{group}</h2>
          {categories
            .filter((c) => c.group === group)
            .map((c) => {
              const value = entry(c.key);
              const cents = Math.max(0, looseCents(value.amount, decimal));
              return (
                <div className="setup-amount" key={c.key}>
                  <label>
                    <span>{c.name}</span>
                    <input
                      inputMode="decimal"
                      dir="ltr"
                      placeholder={editable(0, format)}
                      value={value.amount}
                      onChange={(e) => onAmounts({ ...amounts, [c.key]: { ...value, amount: e.target.value } })}
                    />
                  </label>
                  <select
                    aria-label={t("How often, for {name}", { name: c.name })}
                    value={value.yearly ? "year" : "month"}
                    onChange={(e) => onAmounts({ ...amounts, [c.key]: { ...value, yearly: e.target.value === "year" } })}
                  >
                    <option value="month">{t("a month")}</option>
                    <option value="year">{t("a year")}</option>
                  </select>
                  <small className="setup-per-month">{value.yearly && cents > 0 ? t("{amount} a month", { amount: show(monthly(cents, true), format) }) : ""}</small>
                </div>
              );
            })}
        </section>
      ))}
      <p className="setup-sum">
        {t("Your month costs {cost}. You have {have}.", { cost: <strong>{show(cost, format)}</strong>, have: <strong>{show(have, format)}</strong> })}
      </p>
      <details className="setup-fold" open={!!income.trim() || undefined}>
        <summary>{t("What comes in each month?")}</summary>
        <label>
          {t("Income you expect in a month")}
          <input inputMode="decimal" dir="ltr" placeholder={editable(0, format)} value={income} onChange={(e) => onIncome(e.target.value)} />
          <small>{t("Optional. It lets the budget say whether the month fits what you earn.")}</small>
        </label>
      </details>
    </>
  );
}

/** Why the typed amounts can't go on, or "". */
function amountsProblem(texts: string[], decimal: NumberFormat["decimal"]): string {
  try {
    for (const text of texts) typedCents(text, decimal);
    return "";
  } catch (e) {
    return errorText(e);
  }
}

// ---------- Step 4: giving the money its jobs ----------

type PlanRow = { id: string; name: string; group: string; card: boolean; amount: number };
/** What Underfunded would assign, for the screen: the rows it funds, and the totals its ending is told from. */
type Plan = {
  rows: PlanRow[];
  /** Unassigned now. */
  have: number;
  /** What the month asks for in all, with money for all of it. */
  needed: number;
  /** The category leftover money is offered to, if the budget has it. */
  spare?: { id: string; name: string; assigned: number };
};

function planOf(
  categories: { id: string; name: string; group: string; paymentAccountId?: string | null }[],
  preview: SetupPreview,
  assignedNow: (id: string) => number,
): Plan {
  const rows = categories.flatMap((c) => {
    const change = preview.changes.find((x) => x.id === c.id);
    return change && change.after !== change.before ? [{ id: c.id, name: c.name, group: c.group, card: !!c.paymentAccountId, amount: change.after - change.before }] : [];
  });
  const spare = leftoverCategory(categories.filter((c) => !c.paymentAccountId));
  return {
    rows,
    have: preview.readyToAssign,
    needed: preview.needed,
    spare: spare && { id: spare.id, name: spare.name, assigned: preview.changes.find((x) => x.id === spare.id)?.after ?? assignedNow(spare.id) },
  };
}

/** What a plan assigns in all, what that leaves, and how far short of the month it stops. */
const sums = (plan: Plan) => {
  const total = plan.rows.reduce((sum, row) => sum + row.amount, 0);
  return { total, left: plan.have - total, short: plan.needed - total };
};
/** The leftover that goes to the spare category, when that was asked for and there is one. */
const spared = (plan: Plan, toSpare: boolean) => {
  const { left, short } = sums(plan);
  return toSpare && short <= 0 && left > 0 && plan.spare ? left : 0;
};

/** The edit "Assign it" makes: Underfunded for every category, then the leftover if it was given a home. One step for Undo. */
const fund = (month: string, ids: string[], plan: Plan, leftover: number) => async (budget: Budget) => {
  let next = plan.rows.length ? await api.action(budget, autoAssignAction("underfunded", month, ids, true, month)) : budget;
  if (leftover > 0 && plan.spare)
    next = await api.action(next, { type: "assignAmount", month, categoryId: plan.spare.id, amount: plan.spare.assigned + leftover });
  return next;
};

function AssignPlan({
  plan,
  groups,
  failed,
  toSpare,
  onToSpare,
  format,
}: {
  plan: Plan | null;
  /** Group names in the budget's order. */
  groups: string[];
  /** It couldn't be worked out; the step says why. */
  failed: boolean;
  /** The leftover goes to the spare category too. */
  toSpare: boolean;
  onToSpare: (on: boolean) => void;
  format: Money;
}) {
  if (!plan) return failed ? null : <p className="muted" role="status">{t("Working it out…")}</p>;
  const { total, left, short } = sums(plan);
  const cards = plan.rows.filter((row) => row.card);
  const toCards = cards.reduce((sum, row) => sum + row.amount, 0);
  const sparing = spared(plan, toSpare) > 0;
  return (
    <>
      <div className="setup-plan">
        {groups
          .map((group) => ({ group, rows: plan.rows.filter((row) => row.group === group) }))
          .filter((g) => g.rows.length)
          .map(({ group, rows }) => (
            <details key={group}>
              <summary>
                <span>{group}</span>
                <span className="setup-figure">{show(rows.reduce((sum, row) => sum + row.amount, 0), format)}</span>
              </summary>
              <ul>
                {rows.map((row) => (
                  <li key={row.id}>
                    <span>{row.name}</span>
                    <span className="setup-figure">{show(row.amount, format)}</span>
                  </li>
                ))}
              </ul>
            </details>
          ))}
        {sparing && (
          <p className="setup-plan-line">
            <span>{plan.spare!.name}</span>
            <span className="setup-figure">+{show(left, format)}</span>
          </p>
        )}
        <p className="setup-plan-line total">
          <span>{t("You have")}</span>
          <span className="setup-figure">{show(plan.have, format)}</span>
        </p>
        <p className="setup-plan-line total">
          <span>{t("Left after this")}</span>
          <span className="setup-figure">{show(sparing ? 0 : left, format)}</span>
        </p>
      </div>
      <p className="setup-ending" role="status">
        {short > 0
          ? t("Your month costs {amount} more than you have right now. The rest waits for payday; assign it when it arrives.", { amount: <strong>{show(short, format)}</strong> })
          : left > 0 && !sparing
            ? plan.spare
              ? t("{amount} is left over. Put it in {category}, or keep it for next month.", { amount: <strong>{show(left, format)}</strong>, category: plan.spare.name })
              : t("{amount} is left over. It stays in Unassigned for next month.", { amount: <strong>{show(left, format)}</strong> })
            : total > 0 || sparing
              ? t("All of it has a job.")
              : t("There's nothing to assign yet. Add your accounts and what a month costs, then come back to this step.")}
      </p>
      {short <= 0 && left > 0 && plan.spare && (
        <p>
          <button type="button" className="secondary" aria-pressed={sparing} onClick={() => onToSpare(!sparing)}>
            {sparing ? t("Keep it for next month") : t("Put it in {category}", { category: plan.spare.name })}
          </button>
        </p>
      )}
      {toCards > 0 && (
        <p className="setup-ending">
          {tn(cards.length, "{amount} is set aside to pay your card.", "{amount} is set aside to pay your cards.", { amount: show(toCards, format) })}
        </p>
      )}
    </>
  );
}

// ---------- Step 5: keeping it up to date ----------

function KeepChoice({ value, onChange }: { value: KeepRoute; onChange: (value: KeepRoute) => void }) {
  const toast = useToast();
  const options: [KeepRoute, string, ReactNode][] = [
    ["type", t("Type it in as I spend"), t("Add each purchase when it happens, or catch up once a week.")],
    [
      "file",
      t("Import my bank's file"),
      <>
        {t("About once a week, from your bank's website. Import from tomorrow's transactions on: today's balance already counts the rest.")}{" "}
        <button
          type="button"
          className="link-button"
          onClick={() => void cloud.open("/import/").catch((e) => toast({ message: errorText(e), tone: "error" }))}
        >
          {t("How importing works")}
        </button>
      </>,
    ],
    ["bank", t("Connect my bank"), t("US and Canada, through SimpleFIN, a service you pay for.")],
  ];
  return (
    <fieldset className="choice-list">
      {options.map(([route, title, help]) => (
        <label key={route} className={`choice${value === route ? " selected" : ""}`}>
          <input type="radio" name="keep" value={route} checked={value === route} onChange={() => onChange(route)} />
          <span>
            <strong>{title}</strong>
            <small>{help}</small>
          </span>
        </label>
      ))}
    </fieldset>
  );
}

// ---------- Before the budget exists ----------

type Shared = {
  /** The month the money is assigned in: the current one. */
  month: string;
  /** Switches to a budget (saving the open one first). */
  openBudget: (id: string) => Promise<void>;
  /** A change to the open budget that Undo takes back in one step. */
  transform: (work: (budget: Budget) => Promise<Budget>) => Promise<void>;
  close: () => void;
};

function DraftSetup({ origin, month, openBudget, transform, close, onBudget }: Shared & { origin: SetupOrigin; onBudget: (step: number) => void }) {
  const toast = useToast();
  const language = useLanguage();
  const [draft, setDraft] = useState<SetupDraft>(() => readDraft(origin) ?? newDraft(origin, { name: t("My budget") }));
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const update = (patch: Partial<SetupDraft>) => {
    setError("");
    setDraft((d) => ({ ...d, ...patch }));
  };
  // The answers are kept as they're typed, so closing the app loses nothing.
  const made = useRef(false);
  useEffect(() => {
    if (!made.current) writeDraft(draft);
  }, [draft]);
  const templates = useBudgetTemplates();
  const template = templates?.find((x) => x.id === draft.template);
  const format = { currency: draft.format.currency, numberFormat: draft.format.numberFormat };
  const decimal = format.numberFormat.decimal;
  const categories = useMemo(() => draftCategories(draft, template, t("Other")), [draft.leftOut, draft.added, template, language]);
  const cash = draft.accounts.filter((row) => row.type !== "creditCard" && row.name.trim()).reduce((sum, row) => sum + looseCents(row.balance, decimal), 0);

  // The last step's budget, built once the answers are in, and what Underfunded would give it.
  const [built, setBuilt] = useState<{ budget: Budget; plan: Plan } | null>(null);
  const [toSpare, setToSpare] = useState(false);
  const created = useRef<string | null>(null);
  useEffect(() => {
    if (draft.step !== 4 || made.current) return;
    let active = true;
    setBuilt(null);
    setError("");
    void (async () => {
      const budget = await api.setupBudget(answersOf(draft, 4));
      const preview = await api.setupPreview(budget, month);
      if (active) setBuilt({ budget, plan: planOf(budget.categories, preview, () => 0) });
    })().catch((e) => {
      if (active) setError(errorText(e));
    });
    return () => {
      active = false;
    };
    // The answers can't change on this step; a new language names the budget's categories again.
  }, [draft.step, month, language]);

  /** Saves the budget and opens it. From here on the walkthrough works on that budget. */
  const make = async (budget: Budget) => {
    const id = (created.current ??= await api.createBudget(budget, undefined, draft.localOnly));
    made.current = true;
    writeMark(id, readMark(id) ?? { card: true });
    clearDraft(origin);
    await openBudget(id);
    return id;
  };
  const run = (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError("");
    work()
      .catch((e) => setError(errorText(e)))
      .finally(() => setBusy(false));
  };
  // Skip setup makes the budget from what's been answered up to this step.
  const skip = () =>
    run(async () => {
      await make(draft.step === 4 && built ? built.budget : await api.setupBudget(answersOf(draft)));
      close();
    });
  const assign = () =>
    run(async () => {
      if (!built) return;
      const { budget, plan } = built;
      const leftover = spared(plan, toSpare);
      const id = await make(budget);
      try {
        if (plan.rows.length || leftover > 0) await transform(fund(month, budget.categories.map((c) => c.id), plan, leftover));
        updateMark(id, { assigned: true });
        onBudget(5);
      } catch (e) {
        // The budget is made and open; assigning can be tried again on it.
        toast({ message: errorText(e), tone: "error" });
        onBudget(4);
      }
    });
  const go = (step: SetupDraft["step"]) => update({ step });
  const frame = { busy, skip, error };

  if (draft.step === 1)
    return (
      <Frame
        {...frame}
        step={1}
        title={t("Where is your money?")}
        lead={t("Add each account you spend from, with today's balance. Credit cards too.")}
        label={t("Next")}
        action={() => {
          const problem = accountsProblem(draft.accounts, 0, decimal);
          if (problem) setError(problem);
          else go(2);
        }}
      >
        <AccountRows rows={draft.accounts} onRows={(accounts) => update({ accounts })} format={format} />
      </Frame>
    );
  if (draft.step === 2)
    return (
      <Frame
        {...frame}
        step={2}
        title={t("What do you spend on?")}
        lead={t("Start from a set and make it yours. Click a category to leave it out. You can change all of this later.")}
        label={t("Next")}
        back={() => go(1)}
        blocked={!templates}
        action={() => {
          if (!categories.length) setError(t("Keep or add at least one category to go on."));
          else go(3);
        }}
      >
        <CategoryChoice
          templates={templates}
          templateId={draft.template}
          // Another set has other categories in the same places: what was left out and typed goes with the old one.
          onTemplate={(id) => update({ template: id, leftOut: [], amounts: Object.fromEntries(Object.entries(draft.amounts).filter(([key]) => key.startsWith("+"))) })}
          leftOut={draft.leftOut}
          onLeftOut={(leftOut) => update({ leftOut })}
          added={draft.added}
          onAdded={(added) => update({ added })}
        />
      </Frame>
    );
  if (draft.step === 3)
    return (
      <Frame
        {...frame}
        step={3}
        title={t("What does a month cost?")}
        lead={t("Exact for bills, a guess for the rest. Leave it blank where you're not sure.")}
        label={t("Next")}
        back={() => go(2)}
        action={() => {
          const problem = amountsProblem([...categories.map((c) => draft.amounts[c.key]?.amount ?? ""), draft.income], decimal);
          if (problem) setError(problem);
          else go(4);
        }}
      >
        <AmountRows
          categories={categories}
          amounts={draft.amounts}
          onAmounts={(amounts) => update({ amounts })}
          income={draft.income}
          onIncome={(income) => update({ income })}
          have={cash}
          format={format}
        />
      </Frame>
    );
  return (
    <Frame
      {...frame}
      step={4}
      title={t("Give your money its jobs.")}
      lead={t("Each category gets what its month costs, from the money you have.")}
      label={t("Assign it")}
      back={() => go(3)}
      blocked={!built}
      action={assign}
    >
      <AssignPlan
        plan={built?.plan ?? null}
        groups={built ? groupNames(built.budget) : []}
        failed={!!error}
        toSpare={toSpare}
        onToSpare={setToSpare}
        format={format}
      />
    </Frame>
  );
}

// ---------- On a budget that exists ----------

function BudgetSetup({
  budget,
  budgetId,
  summary,
  edit,
  start,
  month,
  transform,
  close,
}: Shared & {
  budget: Budget;
  budgetId: string;
  /** The month's figures; null while they're being worked out. */
  summary: Summary | null;
  edit: (update: (draft: Budget) => void) => void;
  start: number;
}) {
  const [step, setStep] = useState(Math.min(5, Math.max(1, start)));
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const format = { currency: budget.currency, numberFormat: numberFormatFor(budget.currency, budget.numberFormat) };
  const decimal = format.numberFormat.decimal;
  const go = (next: number) => {
    setError("");
    setStep(next);
  };

  // Step 1: the budget's accounts, and rows for more.
  const [rows, setRows] = useState<AccountRow[]>(() => (budget.accounts.length ? [] : [blankRow()]));
  const balances = new Map(summary?.accounts.map((a) => [a.id, a.balance]));
  const fixed: FixedAccount[] = budget.accounts
    .filter((a) => !a.archived && a.kind !== "tracking")
    .map((a) => ({ id: a.id, name: a.name, type: accountType(a), balance: balances.get(a.id) ?? a.openingBalance }));
  const cash = fixed.filter((a) => a.type !== "creditCard" && a.type !== "lineOfCredit").reduce((sum, a) => sum + a.balance, 0);

  // Step 2: its categories; a template to start from when it has none of its own.
  const templates = useBudgetTemplates();
  const [templateId, setTemplateId] = useState("everyday");
  const [leftOut, setLeftOut] = useState<string[]>([]);
  const [added, setAdded] = useState<string[]>([]);
  const own = ownCategories(budget);
  const existing = own.length
    ? groupNames(budget)
        .map((name) => ({ name, categories: orderedCategories(budget).filter((c) => c.group === name && !c.paymentAccountId && !c.hidden).map((c) => c.name) }))
        .filter((group) => group.categories.length)
    : undefined;
  const adding = draftCategories({ leftOut, added }, existing ? undefined : templates?.find((x) => x.id === templateId), t("Other"));

  // Step 3: a plain monthly amount per category. One with a target of another kind keeps it: the full editor sets those.
  const plain = orderedCategories(budget).filter((c) => !c.paymentAccountId && !c.hidden && !c.goal);
  const rank = new Map(groupNames(budget).map((name, i) => [name, i]));
  const listed: SetupCategory[] = plain
    .map((c) => ({ key: c.id, name: c.name, group: c.group }))
    .sort((a, b) => (rank.get(a.group) ?? 0) - (rank.get(b.group) ?? 0));
  const [typed, setTyped] = useState<Record<string, AmountEntry>>({});
  const amounts: Record<string, AmountEntry> = { ...Object.fromEntries(plain.filter((c) => c.target > 0).map((c) => [c.id, { amount: editable(c.target, format) }])), ...typed };
  const [income, setIncome] = useState<string | null>(null);
  const incomeText = income ?? (budget.expectedIncome ? editable(budget.expectedIncome, format) : "");

  // Step 4: Underfunded on the open budget, as Quick assign previews it.
  const [plan, setPlan] = useState<{ budget: Budget; plan: Plan } | null>(null);
  const [toSpare, setToSpare] = useState(false);
  const ids = useMemo(() => budget.categories.map((c) => c.id), [budget.categories]);
  useEffect(() => {
    if (step !== 4 || !summary) return;
    let active = true;
    setError("");
    void (async () => {
      const options = { asOfMonth: month };
      const changes = async (limit: boolean) => (ids.length ? ((await api.autoAssignPreview(budget, month, ids, limit, options)).underfunded ?? []) : []);
      const [within, all] = [await changes(true), await changes(false)];
      const preview = { readyToAssign: summary.readyToAssign, needed: all.reduce((sum, c) => sum + c.after - c.before, 0), changes: within };
      const assigned = (id: string) => budget.assignments.find((a) => a.month === month && a.categoryId === id)?.amount ?? 0;
      if (active) setPlan({ budget, plan: planOf(budget.categories, preview, assigned) });
    })().catch((e) => {
      if (active) setError(errorText(e));
    });
    return () => {
      active = false;
    };
  }, [step, budget, summary, month, ids]);
  const current = plan?.budget === budget ? plan.plan : null;

  // Step 5: how spending will keep arriving, kept for this budget on this device.
  const [keep, setKeep] = useState<KeepRoute>(() => readMark(budgetId)?.keep ?? "type");

  const frame = { busy, skip: close, error };
  if (step === 1)
    return (
      <Frame
        {...frame}
        step={1}
        title={t("Where is your money?")}
        lead={t("Add each account you spend from, with today's balance. Credit cards too.")}
        label={t("Next")}
        action={() => {
          const problem = accountsProblem(rows, budget.accounts.length, decimal);
          if (problem) return setError(problem);
          if (rows.some((row) => row.name.trim())) edit((d) => addAccounts(d, rows, decimal));
          setRows([]);
          go(2);
        }}
      >
        <AccountRows fixed={fixed} rows={rows} onRows={setRows} format={format} />
      </Frame>
    );
  if (step === 2)
    return (
      <Frame
        {...frame}
        step={2}
        title={t("What do you spend on?")}
        lead={
          existing
            ? t("These are your categories. Add any that are missing.")
            : t("Start from a set and make it yours. Click a category to leave it out. You can change all of this later.")
        }
        label={t("Next")}
        back={() => go(1)}
        blocked={!existing && !templates}
        action={() => {
          if (!own.length && !adding.length) return setError(t("Keep or add at least one category to go on."));
          if (adding.length) edit((d) => addCategories(d, adding));
          setLeftOut([]);
          setAdded([]);
          go(3);
        }}
      >
        <CategoryChoice
          existing={existing}
          templates={templates}
          templateId={templateId}
          onTemplate={(id) => {
            setTemplateId(id);
            setLeftOut([]);
          }}
          leftOut={leftOut}
          onLeftOut={setLeftOut}
          added={added}
          onAdded={setAdded}
        />
      </Frame>
    );
  if (step === 3)
    return (
      <Frame
        {...frame}
        step={3}
        title={t("What does a month cost?")}
        lead={t("Exact for bills, a guess for the rest. Leave it blank where you're not sure.")}
        label={t("Next")}
        back={() => go(2)}
        action={() => {
          const problem = amountsProblem([...listed.map((c) => amounts[c.key]?.amount ?? ""), incomeText], decimal);
          if (problem) return setError(problem);
          const target = (id: string) => monthly(Math.max(0, looseCents(amounts[id]?.amount ?? "", decimal)), amounts[id]?.yearly);
          const expected = Math.max(0, looseCents(incomeText, decimal));
          if (plain.some((c) => c.target !== target(c.id)) || (budget.expectedIncome ?? 0) !== expected)
            edit((d) => {
              for (const c of d.categories) if (plain.some((x) => x.id === c.id)) c.target = target(c.id);
              if (expected > 0) d.expectedIncome = expected;
              else delete d.expectedIncome;
            });
          setTyped({});
          setIncome(null);
          go(4);
        }}
      >
        <AmountRows categories={listed} amounts={amounts} onAmounts={setTyped} income={incomeText} onIncome={setIncome} have={cash} format={format} />
      </Frame>
    );
  if (step === 4)
    return (
      <Frame
        {...frame}
        step={4}
        title={t("Give your money its jobs.")}
        lead={t("Each category gets what its month costs, from the money you have.")}
        label={t("Assign it")}
        back={() => go(3)}
        blocked={!current}
        action={() => {
          if (!current) return;
          const leftover = spared(current, toSpare);
          setError("");
          setBusy(true);
          void (current.rows.length || leftover > 0 ? transform(fund(month, ids, current, leftover)) : Promise.resolve())
            .then(() => {
              updateMark(budgetId, { assigned: true });
              go(5);
            })
            .catch((e) => setError(errorText(e)))
            .finally(() => setBusy(false));
        }}
      >
        <AssignPlan plan={current} groups={groupNames(budget)} failed={!!error} toSpare={toSpare} onToSpare={setToSpare} format={format} />
      </Frame>
    );
  return (
    <Frame
      {...frame}
      step={5}
      title={t("How will you keep it up to date?")}
      lead={t("Pick the one you'll really do. Nothing is connected or imported here, and you can change your mind any time.")}
      label={t("Finish")}
      back={() => go(4)}
      action={() => {
        updateMark(budgetId, { keep });
        close();
      }}
    >
      <KeepChoice value={keep} onChange={setKeep} />
    </Frame>
  );
}

/** The walkthrough, as the whole window's page. */
export function SetupWalkthrough({
  session,
  onSession,
  budget,
  budgetId,
  summary,
  edit,
  ...shared
}: Shared & {
  session: SetupSession;
  /** Moves a draft's walkthrough onto the budget it made. */
  onSession: (session: SetupSession) => void;
  budget: Budget | null;
  budgetId: string;
  summary: Summary | null;
  edit: (update: (draft: Budget) => void) => void;
}) {
  if (session.mode === "draft") return <DraftSetup key={session.origin} origin={session.origin} onBudget={(step) => onSession({ mode: "budget", step })} {...shared} />;
  if (!budget)
    return (
      <p className="setup muted" role="status">
        {t("Opening your budget…")}
      </p>
    );
  return <BudgetSetup key={budgetId} budget={budget} budgetId={budgetId} summary={summary} edit={edit} start={session.step} {...shared} />;
}

/**
 * On Budget, for a budget the walkthrough made and didn't finish: how many steps are left, and
 * the way back in at the first of them. Hidden with one click, for good.
 */
export function SetupCard({ budget, budgetId, unassigned, onContinue }: { budget: Budget; budgetId: string; unassigned: number; onContinue: (step: number) => void }) {
  const [mark, setMark] = useState(() => readMark(budgetId));
  useEffect(() => {
    setMark(readMark(budgetId));
    const changed = (e: Event) => {
      if ((e as CustomEvent<string>).detail === budgetId) setMark(readMark(budgetId));
    };
    window.addEventListener(MARK_EVENT, changed);
    return () => window.removeEventListener(MARK_EVENT, changed);
  }, [budgetId]);
  if (!mark?.card || budget.sample || budget.importedFrom || budget.ynabPlanId) return null;
  const left = stepsLeft(budget, unassigned, mark);
  if (!left.length) return null;
  return (
    <section className="setup-card" aria-label={t("Finish setting up")}>
      <span>
        <strong>{t("Finish setting up")}</strong>
        <span aria-hidden> · </span>
        {tn(left.length, "{count} step left", "{count} steps left")}
      </span>
      <button className="primary small" onClick={() => onContinue(left[0])}>
        {t("Continue")}
      </button>
      <button className="icon-button" title={t("Hide setup")} aria-label={t("Hide setup")} onClick={() => writeMark(budgetId, { ...mark, card: false })}>
        <X size={15} />
      </button>
    </section>
  );
}
