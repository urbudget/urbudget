import { useState } from "react";

/**
 * The accounts and categories the reports leave out, remembered per budget on this computer, so
 * leaving out a category such as stock purchases holds every time Reports opens. Kept as what is
 * hidden rather than what is shown, so a new account or category appears in reports until it is
 * hidden too.
 */
export type Hidden = { accounts: Set<string>; categories: Set<string> };

const key = (budgetId: string) => `lb:reports:hidden:${budgetId}`;
const ids = (value: unknown) => new Set(Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : []);

function load(budgetId: string): Hidden {
  try {
    const saved = JSON.parse(localStorage.getItem(key(budgetId)) ?? "null");
    return { accounts: ids(saved?.accounts), categories: ids(saved?.categories) };
  } catch {
    return { accounts: new Set(), categories: new Set() };
  }
}

/** What this budget's reports leave out, and a setter that remembers it. */
export function useHidden(budgetId: string) {
  const [state, setState] = useState(() => ({ budgetId, hidden: load(budgetId) }));
  let { hidden } = state;
  if (state.budgetId !== budgetId) {
    // Another budget opened: its reports leave out what it left out.
    hidden = load(budgetId);
    setState({ budgetId, hidden });
  }
  const change = (next: Partial<Hidden>) => {
    const value = { ...hidden, ...next };
    setState({ budgetId, hidden: value });
    try {
      localStorage.setItem(key(budgetId), JSON.stringify({ accounts: [...value.accounts], categories: [...value.categories] }));
    } catch {
      // Remembered until Reports closes.
    }
  };
  return [hidden, change] as const;
}

/** The chosen ids a report filter sends: empty for all of `ids`, `[""]` for none of them. */
export function chosen(hidden: Set<string>, ids: string[]): Set<string> {
  const shown = ids.filter((id) => !hidden.has(id));
  return shown.length === ids.length ? new Set() : new Set(shown.length ? shown : [""]);
}
