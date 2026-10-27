import { useCallback, useState } from "react";
import { Modal } from "./forms";
import { groupNames } from "./categories";
import { addCategory } from "./budgetEdits";
import type { Budget } from "./types";
import { t } from "./i18n";

type Edit = (fn: (b: Budget) => void) => void;
const NEW_GROUP = "\u0000new";

/** Groups a new spending category can go in: every group but the credit card payments one. */
export function spendingGroups(b: Pick<Budget, "categories" | "groupOrder">) {
  const ordinary = new Set(b.categories.filter((c) => !c.paymentAccountId).map((c) => c.group));
  const payment = new Set(b.categories.filter((c) => c.paymentAccountId).map((c) => c.group));
  return groupNames(b, { empty: true }).filter((g) => ordinary.has(g) || !payment.has(g));
}

/** The group the last category created this way went in, offered first next time. */
let lastGroup = "";

/** Names a category typed into a category picker and asks which group it belongs in. */
export function NewCategoryDialog({
  name,
  groups,
  close,
  create,
}: {
  name: string;
  groups: string[];
  close: () => void;
  create: (name: string, group: string) => void;
}) {
  const [text, setText] = useState(name);
  const [group, setGroup] = useState(groups.includes(lastGroup) ? lastGroup : (groups[0] ?? NEW_GROUP));
  const [newGroup, setNewGroup] = useState("");
  const [error, setError] = useState("");
  const adding = group === NEW_GROUP;
  return (
    <Modal title={t("New category")} close={close}>
      <form
        className="new-category"
        onSubmit={(e) => {
          e.preventDefault();
          const chosen = adding ? newGroup.trim() : group;
          if (!text.trim()) return setError(t("Enter a name for the category."));
          if (!chosen) return setError(t("Name the new group."));
          lastGroup = chosen;
          create(text.trim(), chosen);
        }}
      >
        <label>
          {t("Name")}
          <input data-autofocus required maxLength={200} value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        <label>
          {t("Group")}
          <select value={group} onChange={(e) => setGroup(e.target.value)}>
            {groups.map((g) => (
              <option key={g} value={g}>
                {g}
              </option>
            ))}
            <option value={NEW_GROUP}>{t("New group…")}</option>
          </select>
        </label>
        {adding && (
          <label>
            {t("New group")}
            <input autoFocus maxLength={200} placeholder={t("e.g. Pets")} value={newGroup} onChange={(e) => setNewGroup(e.target.value)} />
          </label>
        )}
        <p className="help">{t("It starts with nothing assigned. Give it money or a target from the budget.")}</p>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary">{t("Create category")}</button>
        </div>
      </form>
    </Modal>
  );
}

/**
 * Creating a category from a category picker. `create(name)` asks for its group, adds it, and
 * resolves to the new category's id, or null when the user cancels. Render `dialog` outside the
 * editor that asked, so its Enter and Escape stay with the dialog.
 */
export function useCategoryCreator(budget: Budget | null, edit: Edit) {
  const [pending, setPending] = useState<{ name: string; done: (id: string | null) => void } | null>(null);
  const create = useCallback((name: string) => new Promise<string | null>((done) => setPending({ name, done })), []);
  const dialog = pending && budget && (
    <NewCategoryDialog
      name={pending.name}
      groups={spendingGroups(budget)}
      close={() => {
        pending.done(null);
        setPending(null);
      }}
      create={(name, group) => {
        let added = "";
        edit((d) => {
          added = addCategory(d, group, name);
        });
        pending.done(added || null);
        setPending(null);
      }}
    />
  );
  return { create, dialog };
}
