import type { Budget, MoneyMove } from "./types";
import { AUTO_ASSIGN } from "./QuickAssign";
import { getLanguage, t } from "./i18n";

/** One line of a category's moves for a month. */
export type CategoryMove = {
  key: string;
  /** When it happened; null for the line that stands for changes that were not itemised. */
  at: string | null;
  what: string;
  detail?: string;
  /** The change to the category's Assigned. */
  amount: number;
};

const changeTo = (m: MoneyMove, categoryId: string) =>
  m.changes?.find((c) => c.categoryId === categoryId)?.amount ?? 0;

/** A quick-assign option as the engine records it ("spent last month"), in the language in use. */
const optionName = (detail: string) => {
  if (getLanguage() === "en") return detail;
  const option = AUTO_ASSIGN.find((o) => o.label.toLowerCase() === detail.toLowerCase());
  return option ? t(option.label) : detail;
};

/** Names a move from one category's side: where its money came from or went. */
function describe(m: MoneyMove, categoryId: string, amount: number, names: Map<string, string>) {
  const other = m.changes?.find((c) => c.categoryId !== categoryId);
  if (m.kind === "quickAssign") return { what: m.detail ? t("Quick assign: {option}", { option: optionName(m.detail) }) : t("Quick assign") };
  if (m.kind === "merge")
    return { what: m.detail ? t("Merged from {name}", { name: m.detail }) : t("Merged from a deleted category") };
  if (m.kind === "move" && other) {
    const name = names.get(other.categoryId);
    if (name === undefined) return { what: amount > 0 ? t("Moved from a deleted category") : t("Moved to a deleted category") };
    return { what: amount > 0 ? t("Moved from {name}", { name }) : t("Moved to {name}", { name }) };
  }
  // Typed amounts, and moves to or from Unassigned.
  if (m.kind === "move" || m.kind === "assign") return { what: amount > 0 ? t("Assigned") : t("Returned to Unassigned") };
  // The engine's own record, kept as it was written.
  return { what: m.description };
}

/**
 * A category's moves in a month, oldest first: each recorded change to its Assigned, named by
 * where the money came from or went. When they don't add up to `assigned` (changes made before
 * moves were itemised, or an import), a first line makes up the difference, so the list always
 * adds up to the month's Assigned.
 */
export function categoryMoves(b: Budget, categoryId: string, month: string, assigned: number): CategoryMove[] {
  const names = new Map(b.categories.map((c) => [c.id, c.name]));
  const lines: CategoryMove[] = [];
  (b.moneyMoves ?? []).forEach((m, i) => {
    const amount = m.month === month ? changeTo(m, categoryId) : 0;
    if (amount) lines.push({ key: String(i), at: m.at, amount, ...describe(m, categoryId, amount, names) });
  });
  const recorded = lines.reduce((n, l) => n + l.amount, 0);
  if (lines.length && recorded !== assigned)
    lines.unshift({ key: "earlier", at: null, what: t("Earlier changes"), detail: t("Not itemised"), amount: assigned - recorded });
  return lines;
}

/**
 * For each category, how many itemised moves it had in the month, and whether money moved
 * between it and another category (which the budget row marks).
 */
export function monthMoveStats(b: Budget, month: string) {
  const stats = new Map<string, { count: number; moved: boolean }>();
  for (const m of b.moneyMoves ?? []) {
    if (m.month !== month) continue;
    const between = m.kind === "merge" || (m.kind === "move" && (m.changes?.length ?? 0) > 1);
    for (const c of m.changes ?? []) {
      if (!c.amount) continue;
      const s = stats.get(c.categoryId) ?? { count: 0, moved: false };
      stats.set(c.categoryId, { count: s.count + 1, moved: s.moved || between });
    }
  }
  return stats;
}
