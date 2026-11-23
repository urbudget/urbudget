import { useState, type RefObject } from "react";
import { CategoryPicker, Popover, READY_TO_ASSIGN } from "./ui";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import type { Budget } from "./types";
import { AppliedChangeError } from "./useBudget";
import { t } from "./i18n";

/**
 * What covering an overspending of `overspent` takes from a source that has `has` (both in cents):
 * all of it when the source has enough, otherwise everything the source has. With nothing to give
 * (or no source chosen) it's the whole overspending, so the amount still says what's needed.
 */
export function coverAmount(overspent: number, has: number | null | undefined): number {
  return has != null && has > 0 && has < overspent ? has : overspent;
}

/**
 * Move funds into or out of a category (or assign unassigned money).
 * `source` is a category id or READY_TO_ASSIGN; `available` is its balance. Assigning unassigned
 * money opens with the amount empty, not with all of it for one category. Covering an overspent
 * category opens with as much of the overspending as the place it's taken from can give
 * (coverAmount), and choosing another place sets the amount again, until one has been typed.
 */
export function MoveMoneyPopover({
  budget,
  anchor,
  source,
  sourceName,
  available,
  balances,
  readyToAssign,
  onMove,
  onClose,
}: {
  budget: Budget;
  anchor: RefObject<HTMLElement | null>;
  source: string;
  sourceName: string;
  available: number;
  balances: Map<string, number>;
  readyToAssign: number;
  onMove: (from: string, to: string, amount: number) => Promise<void>;
  onClose: () => void;
}) {
  const fromReady = source === READY_TO_ASSIGN;
  const withReady = new Map(balances).set(READY_TO_ASSIGN, readyToAssign);
  // Covering this category's overspending: the amount is what the place it comes from can give of it.
  const overspent = available < 0 && !fromReady ? -available : 0;
  const covered = (from: string | null) => coverAmount(overspent, from == null ? null : withReady.get(from) ?? 0);
  const [direction, setDirection] = useState<"out" | "in">(available < 0 ? "in" : "out");
  const [other, setOther] = useState<string | null>(available < 0 ? READY_TO_ASSIGN : null);
  // Assigning from Unassigned starts empty: all of it rarely belongs in one category.
  const [amount, setAmount] = useState(() =>
    fromReady && available >= 0 ? "" : editableAmount(overspent > 0 ? covered(available < 0 ? READY_TO_ASSIGN : null) : Math.abs(available)));
  // Until an amount is typed, a new place to take from puts in what it can give.
  const [typed, setTyped] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [retrySave, setRetrySave] = useState<(() => Promise<void>) | null>(null);
  const choose = (id: string | null) => {
    setOther(id);
    if (overspent > 0 && direction === "in" && !typed) setAmount(editableAmount(covered(id)));
  };
  const title = fromReady
    ? t("Assign money")
    : available < 0 && direction === "in"
      ? t("Cover overspending in {name}", { name: sourceName })
      : direction === "out"
        ? t("Move funds from {name}", { name: sourceName })
        : t("Move funds to {name}", { name: sourceName });
  return (
    <Popover open onClose={onClose} anchor={anchor} placement="bottom-end" label={title} className="money-popover">
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          try {
            if (retrySave) {
              setBusy(true);
              await retrySave();
              onClose();
              return;
            }
            const cents = evaluateAmount(amount);
            if (cents <= 0) throw new Error(t("Enter an amount above zero."));
            if (!other) throw new Error(direction === "out" ? t("Choose where the money goes.") : t("Choose where the money comes from."));
            setBusy(true);
            await onMove(direction === "out" ? source : other, direction === "out" ? other : source, cents);
            onClose();
          } catch (err) {
            if (err instanceof AppliedChangeError) setRetrySave(() => err.retrySave);
            setError(errorText(err));
            setBusy(false);
          }
        }}
      >
        <strong className="popover-title">{title}</strong>
        {/* The amount starts empty, so say how much there is to give out. */}
        {fromReady && readyToAssign > 0 && (
          <small className="popover-hint">{t("{amount} unassigned", { amount: money(readyToAssign) })}</small>
        )}
        <fieldset disabled={busy || !!retrySave} style={{ border: 0, padding: 0, margin: 0, minWidth: 0, display: "grid", gap: 12 }}>
        {!fromReady && (
          <div className="segmented small" role="radiogroup" aria-label={t("Direction")}>
            <button type="button" role="radio" aria-checked={direction === "out"} className={direction === "out" ? "selected" : ""} onClick={() => setDirection("out")}>
              {t("Move to…")}
            </button>
            <button type="button" role="radio" aria-checked={direction === "in"} className={direction === "in" ? "selected" : ""} onClick={() => setDirection("in")}>
              {available < 0 ? t("Cover from…") : t("Take from…")}
            </button>
          </div>
        )}
        <label>
          {t("Amount")}
          <input
            autoFocus
            inputMode="decimal"
            placeholder="0.00"
            value={amount}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => { setAmount(e.target.value); setTyped(true); }}
          />
        </label>
        <label>
          {direction === "out" ? t("To") : t("From")}
          <CategoryPicker
            budget={budget}
            value={other}
            onChange={choose}
            available={withReady}
            readyToAssign={fromReady ? false : t("Unassigned ({amount})", { amount: money(readyToAssign) })}
            exclude={fromReady ? undefined : source}
            label={direction === "out" ? t("Move to") : t("Take from")}
          />
        </label>
        </fieldset>
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={onClose}>
            {retrySave ? t("Close (draft kept)") : t("Cancel")}
          </button>
          <button className="primary" disabled={busy}>
            {retrySave ? t("Retry saving") : fromReady ? t("Assign") : t("Move")}
          </button>
        </div>
      </form>
    </Popover>
  );
}
