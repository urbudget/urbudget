// The category templates a new budget can start with (budget_templates.rs in the engine), as a
// select with the chosen one's description and groups under it.
import { useEffect, useState } from "react";
import { api } from "./bridge";
import { getLanguage, t, useLanguage, type Language } from "./i18n";
import type { BudgetTemplate } from "./types";

/** The template a new budget starts on, the engine's first. */
export const DEFAULT_TEMPLATE = "everyday";
/** The template with no categories. */
export const EMPTY_TEMPLATE = "empty";

/** The templates in the language in use; null while they load (or if they can't). */
export function useBudgetTemplates(): BudgetTemplate[] | null {
  const language = useLanguage();
  const [loaded, setLoaded] = useState<{ language: Language; list: BudgetTemplate[] } | null>(null);
  useEffect(() => {
    let active = true;
    api.budgetTemplates().then((list) => { if (active) setLoaded({ language: getLanguage(), list }); }, () => {});
    return () => { active = false; };
  }, [language]);
  return loaded?.language === language ? loaded.list : null;
}

/** The chosen template, asked when the budget is made so its names are in the language in use. */
export async function chosenTemplate(id: string): Promise<BudgetTemplate | undefined> {
  const template = (await api.budgetTemplates()).find((x) => x.id === id);
  return template?.groups.length ? template : undefined;
}

/**
 * The select, captioned "Categories" unless what it sits in already says so. Not a label: it can
 * sit inside a choice's label.
 */
export function TemplateSelect({ value, onChange, templates, withEmpty = false, caption = true }: {
  value: string;
  onChange: (id: string) => void;
  templates: BudgetTemplate[] | null;
  /** Lists "No categories" too, where nothing else offers an empty start. */
  withEmpty?: boolean;
  caption?: boolean;
}) {
  const shown = (templates ?? []).filter((x) => withEmpty || x.id !== EMPTY_TEMPLATE);
  const chosen = shown.find((x) => x.id === value);
  return (
    <div className="field template-select">
      {caption && <span>{t("Categories")}</span>}
      <select aria-label={t("Categories")} value={value} disabled={!templates} onChange={(e) => onChange(e.target.value)}>
        {!templates && <option value={value}>{t("Loading…")}</option>}
        {shown.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
      </select>
      {chosen && (
        <p className="template-preview">
          <span>{chosen.description}</span>
          {chosen.groups.length > 0 && <small>{chosen.summary}</small>}
        </p>
      )}
    </div>
  );
}
