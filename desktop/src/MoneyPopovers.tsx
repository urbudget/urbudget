import { useState, type RefObject } from "react";
import { CategoryPicker, Popover, READY_TO_ASSIGN } from "./ui";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import type { Budget } from "./types";
import { AppliedChangeError } from "./useBudget";
import { t } from "./i18n";

/**
 * Move funds into or out of a category (or assign unassigned money).
 * `source` is a category id or READY_TO_ASSIGN; `available` is its balance.
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
  const [direction, setDirection] = useState<"out" | "in">(available < 0 ? "in" : "out");
  const [other, setOther] = useState<string | null>(available < 0 ? READY_TO_ASSIGN : null);
  const [amount, setAmount] = useState(editableAmount(Math.abs(available)));
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [retrySave, setRetrySave] = useState<(() => Promise<void>) | null>(null);
  const withReady = new Map(balances).set(READY_TO_ASSIGN, readyToAssign);
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
            value={amount}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setAmount(e.target.value)}
          />
        </label>
        <label>
          {direction === "out" ? t("To") : t("From")}
          <CategoryPicker
            budget={budget}
            value={other}
            onChange={setOther}
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
