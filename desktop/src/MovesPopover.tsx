import { useMemo } from "react";
import { ArrowLeftRight } from "lucide-react";
import { Popover, type Anchor } from "./ui";
import { money, monthName } from "./format";
import { categoryMoves, type CategoryMove } from "./moves";
import type { Budget } from "./types";
import { localDateTime, monthNames, t, useLanguage } from "./i18n";

/** A move's day, as "15 Oct" (Arabic names the month in full). */
export const moveDay = (at: string) => {
  const date = new Date(at);
  const months = monthNames();
  return months
    ? `${date.getDate()} ${months[date.getMonth()]}`
    : date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
};
/** When exactly, for a tooltip. */
const when = (at: string) => localDateTime(new Date(at), { dateStyle: "medium", timeStyle: "short" });
const signed = (n: number) => `${n < 0 ? "−" : "+"}${money(Math.abs(n))}`;

/** A category's moves: when, what (where the money came from or went), and the change. */
export function MoveList({ lines, label }: { lines: CategoryMove[]; label: string }) {
  return (
    <ul className="move-list" aria-label={label}>
      {lines.map((l) => (
        <li key={l.key} className={l.at ? undefined : "earlier"}>
          {l.at ? (
            <time dateTime={l.at} title={when(l.at)}>
              {moveDay(l.at)}
            </time>
          ) : (
            <span />
          )}
          <span>
            {l.what}
            {l.detail && <small>{l.detail}</small>}
          </span>
          <span className="move-amount">{signed(l.amount)}</span>
        </li>
      ))}
    </ul>
  );
}

/** The moves behind a category's Assigned for the month, adding up to it. */
export function MovesPopover({
  budget,
  categoryId,
  categoryName,
  month,
  assigned,
  anchor,
  onClose,
  onMoveFunds,
}: {
  budget: Budget;
  categoryId: string;
  categoryName: string;
  month: string;
  assigned: number;
  anchor: Anchor;
  onClose: () => void;
  onMoveFunds: () => void;
}) {
  // The lines' words are in the language in use.
  const language = useLanguage();
  const lines = useMemo(
    () => categoryMoves(budget, categoryId, month, assigned),
    [budget.moneyMoves, budget.categories, categoryId, month, assigned, language],
  );
  return (
    <Popover open onClose={onClose} anchor={anchor} placement="bottom-end" label={t("{name} moves", { name: categoryName })} className="activity-popover moves-popover">
      <div className="activity-head">
        <strong>{categoryName}</strong>
        <span className="muted">{t("Moves in {month}", { month: monthName(month) })}</span>
      </div>
      {lines.length ? (
        <MoveList lines={lines} label={t("{name} moves", { name: categoryName })} />
      ) : (
        <p className="muted activity-empty">{t("Nothing assigned or moved in this category this month.")}</p>
      )}
      <div className="activity-foot">
        <span>
          {t("Assigned · {amount}", { amount: <strong>{money(assigned)}</strong> })}
        </span>
        <button className="link-button" data-autofocus onClick={onMoveFunds}>
          <ArrowLeftRight size={13} /> {t("Move funds…")}
        </button>
      </div>
    </Popover>
  );
}
