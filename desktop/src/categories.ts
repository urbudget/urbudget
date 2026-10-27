import type { Budget, Category } from "./types";
import { msg } from "./i18n";

/** Display-only group for hidden categories. It is never saved as a category's group; show it with t(). */
export const HIDDEN_GROUP = msg("Hidden categories");
export const groupOf = (c: Category) => (c.hidden ? HIDDEN_GROUP : c.group);
const LAST = Number.MAX_SAFE_INTEGER;

/** Categories in display order: saved order first, then any not in it, in creation order. */
export function orderedCategories(b: Pick<Budget, "categories" | "categoryOrder">) {
  const rank = new Map((b.categoryOrder ?? []).map((id, i) => [id, i]));
  return b.categories
    .map((c, i) => ({ c, i, r: rank.get(c.id) ?? LAST }))
    .sort((x, y) => x.r - y.r || x.i - y.i)
    .map((x) => x.c);
}

/**
 * Real group names in display order (never the hidden pseudo-group). With `empty`, groups
 * that exist only in the saved order (created but not yet given a category) are included.
 */
export function groupNames(b: Pick<Budget, "categories" | "groupOrder">, { empty = false } = {}) {
  const rank = new Map((b.groupOrder ?? []).map((g, i) => [g, i]));
  const names = b.categories.map((c) => c.group);
  if (empty) names.push(...(b.groupOrder ?? []).filter((g) => g !== HIDDEN_GROUP));
  return [...new Set(names)]
    .map((name, i) => ({ name, i, r: rank.get(name) ?? LAST }))
    .sort((x, y) => x.r - y.r || x.i - y.i)
    .map((x) => x.name);
}

/** Groups with their visible categories in order; hidden categories last, in their own group. */
export function orderedGroups(
  b: Pick<Budget, "categories" | "categoryOrder" | "groupOrder">,
  { hidden = true, empty = false }: { hidden?: boolean; empty?: boolean } = {},
) {
  const ordered = orderedCategories(b);
  const groups = groupNames(b, { empty })
    .map((name) => ({ name, categories: ordered.filter((c) => !c.hidden && c.group === name) }))
    // A group whose categories are all hidden only appears under "Hidden categories".
    .filter((g) => g.categories.length || (empty && !b.categories.some((c) => c.group === g.name)));
  const hiddenCategories = ordered.filter((c) => c.hidden);
  if (hidden && hiddenCategories.length)
    groups.push({ name: HIDDEN_GROUP, categories: hiddenCategories });
  return groups;
}
