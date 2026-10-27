import { useEffect, useState, type FormEvent } from "react";
import { Pencil, Plus } from "lucide-react";
import { TargetFields, type TargetMonthContext } from "./TargetFields";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import { today, type Budget, type Category } from "./types";
import { t } from "./i18n";

type Edit = (fn: (b: Budget) => void) => void;
const newGoal = (): Record<string, unknown> => ({
  goal_type: "NEED",
  goal_cadence: 1,
  goal_cadence_frequency: 1,
  goal_needs_whole_amount: true,
  goal_creation_month: `${today().slice(0, 7)}-01`,
});

/**
 * Create, change or remove a category's target in the inspector. Saves the same shape as the
 * category dialog: an unchanged amount and type keep the original (possibly imported) goal.
 */
export function TargetEditor({
  category,
  edit,
  open,
  onOpenChange,
  onSaved,
  month,
  currentMonth,
}: {
  category: Category;
  edit: Edit;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: (message: string) => void;
} & TargetMonthContext) {
  const [amount, setAmount] = useState("");
  const [goal, setGoal] = useState<Record<string, unknown>>({});
  const [changed, setChanged] = useState(false);
  const [error, setError] = useState("");
  const has = category.target > 0;
  useEffect(() => {
    if (!open) return;
    setAmount(has ? editableAmount(category.target) : "");
    setGoal(category.goal ?? newGoal());
    setChanged(false);
    setError("");
  }, [open, category.id]);
  const update = (fn: (c: Category) => void) =>
    edit((d) => {
      const c = d.categories.find((x) => x.id === category.id);
      if (c) fn(c);
    });
  const save = (e: FormEvent) => {
    e.preventDefault();
    try {
      const value = amount.trim() ? evaluateAmount(amount) : 0;
      if (value < 0) throw new Error(t("Targets can't be negative."));
      if (!value) throw new Error(t("Enter a target amount, or remove the target."));
      const keep = value === category.target && !changed && category.goal;
      update((c) => {
        c.target = value;
        c.goal = keep ? category.goal : { ...goal, goal_target: value * 10 };
      });
      onOpenChange(false);
      onSaved(has ? t("Updated the target for {name}.", { name: category.name }) : t("Set the target for {name}.", { name: category.name }));
    } catch (err) {
      setError(errorText(err));
    }
  };
  if (!open)
    return (
      <button className="secondary" onClick={() => onOpenChange(true)}>
        {has ? <Pencil size={14} /> : <Plus size={14} />} {has ? t("Edit target") : t("Create target")}
      </button>
    );
  return (
    <form className="target-editor" onSubmit={save} aria-label={t("Target for {name}", { name: category.name })}>
      <h3>{has ? t("Edit target") : t("Create target")}</h3>
      <label>
        {t("Amount")}
        <input
          autoFocus
          inputMode="decimal"
          placeholder="0.00"
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          title={t("You can type math, like 1200/12")}
        />
      </label>
      <TargetFields
        goal={goal}
        month={month}
        currentMonth={currentMonth}
        paymentCategory={!!category.paymentAccountId}
        onChange={(g) => {
          setGoal(g);
          setChanged(true);
        }}
      />
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="target-actions">
        {has && (
          <button
            type="button"
            className="link-button danger"
            onClick={() => {
              update((c) => {
                c.target = 0;
                c.goal = null;
              });
              onOpenChange(false);
              onSaved(t("Removed the {amount} target from {name}.", { amount: money(category.target), name: category.name }));
            }}
          >
            {t("Remove target")}
          </button>
        )}
        <button type="button" className="secondary small" onClick={() => onOpenChange(false)}>
          {t("Cancel")}
        </button>
        <button className="primary small">{t("Save target")}</button>
      </div>
    </form>
  );
}

/** The category's notes, saved when the field loses focus. */
export function CategoryNotes({ category, edit }: { category: Category; edit: Edit }) {
  const [text, setText] = useState(category.note ?? "");
  useEffect(() => setText(category.note ?? ""), [category.id, category.note]);
  return (
    <label className="category-notes">
      {t("Notes")}
      <textarea
        rows={3}
        placeholder={t("Anything to remember about this category")}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          if (text === (category.note ?? "")) return;
          edit((d) => {
            const c = d.categories.find((x) => x.id === category.id);
            if (c) c.note = text || null;
          });
        }}
      />
    </label>
  );
}
