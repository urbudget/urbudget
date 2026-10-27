import { useState } from "react";
import { CreditCard } from "lucide-react";
import { Modal } from "./forms";
import { DateInput } from "./ui";
import { errorText, evaluateAmount, money } from "./format";
import { getPref } from "./prefs";
import { AppliedChangeError } from "./useBudget";
import { id, today, type Account, type Budget, type Transaction } from "./types";
import { t } from "./i18n";

type Choice = "aside" | "balance" | "other";

/**
 * Paying a credit card: a transfer from a budget account to the card, for the money set aside in
 * its payment category, the whole balance, or another amount. As a transfer it isn't spending: it
 * uses up the payment category and lowers what the card owes.
 */
export function CardPaymentForm({
  budget,
  card,
  balances,
  available,
  close,
  pay,
}: {
  budget: Budget;
  card: Account;
  /** Working balances by account id. */
  balances: Map<string, { balance: number }>;
  /** This month's available amounts by category id. */
  available?: Map<string, number>;
  close: () => void;
  pay: (t: Transaction) => Promise<void>;
}) {
  const category = budget.categories.find((c) => c.paymentAccountId === card.id);
  // A failed save may already be reflected in the recovery draft and its balances. Keep the
  // amount choices and identity of this payment stable so retry saves exactly one payment.
  const [owed] = useState(() => Math.max(0, -(balances.get(card.id)?.balance ?? 0)));
  const [aside] = useState(() => Math.max(0, category ? (available?.get(category.id) ?? 0) : 0));
  const [paymentId] = useState(id);
  const sources = budget.accounts.filter((a) => a.kind === "cash" && !a.archived);
  const balanceOf = (a: Account) => balances.get(a.id)?.balance ?? 0;
  // The account that paid this card last; else checking, then anything but savings, then the most in it.
  const [from, setFrom] = useState(() => {
    const last = budget.transactions
      .filter((t) => t.transferAccountId === card.id && t.amount < 0 && sources.some((a) => a.id === t.accountId))
      .reduce<Transaction | null>((latest, t) => (!latest || t.date > latest.date ? t : latest), null);
    if (last) return last.accountId;
    const rank = (a: Account) => (a.accountType === "checking" ? 0 : a.accountType === "savings" || /saving/i.test(a.name) ? 2 : 1);
    return [...sources].sort((a, b) => rank(a) - rank(b) || balanceOf(b) - balanceOf(a))[0]?.id ?? "";
  });
  const [choice, setChoice] = useState<Choice>(aside > 0 ? "aside" : owed > 0 ? "balance" : "other");
  const [other, setOther] = useState("");
  const [date, setDate] = useState(today());
  const [memo, setMemo] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [retrySave, setRetrySave] = useState<(() => Promise<void>) | null>(null);
  const amount = () => (choice === "aside" ? aside : choice === "balance" ? owed : evaluateAmount(other || "0"));
  let shown = 0;
  try {
    shown = amount();
  } catch {
    // Shown as 0.00 until the other amount reads as money.
  }
  const source = sources.find((a) => a.id === from);
  const option = (value: Choice, title: string, detail: string, figure?: number) => (
    <label className={`pay-option${choice === value ? " chosen" : ""}`}>
      <input type="radio" name="pay-amount" checked={choice === value} onChange={() => setChoice(value)} />
      <span>
        <strong>{title}</strong>
        <small>{detail}</small>
      </span>
      {figure !== undefined && <b className="num">{money(figure)}</b>}
    </label>
  );
  return (
    <Modal title={t("Pay {card}", { card: card.name })} close={close}>
      <form
        className="card-payment"
        onSubmit={async (e) => {
          e.preventDefault();
          if (busy) return;
          try {
            if (retrySave) {
              setBusy(true);
              await retrySave();
              close();
              return;
            }
            const value = amount();
            if (!source) throw new Error(t("Choose the account the payment comes from."));
            if (value <= 0) throw new Error(t("Enter an amount to pay."));
            if (!date) throw new Error(t("Choose a date."));
            setBusy(true);
            setError("");
            await pay({
              id: paymentId,
              accountId: source.id,
              date,
              payee: "",
              memo,
              amount: -value,
              status: getPref("newCleared") ? "cleared" : "uncleared",
              approved: true,
              transferAccountId: card.id,
              transferStatus: "uncleared",
              transferSource: { memo },
              splits: [],
            });
          } catch (err) {
            if (err instanceof AppliedChangeError) setRetrySave(() => err.retrySave);
            setError(errorText(err));
            setBusy(false);
          }
        }}
      >
        <div className="pay-facts">
          <div>
            <span>{t("Owed on the card")}</span>
            <strong className="num">{money(owed)}</strong>
          </div>
          <div>
            <span>{t("Set aside to pay it")}</span>
            <strong className="num">{category ? money(aside) : "—"}</strong>
          </div>
        </div>
        <fieldset disabled={busy || !!retrySave} style={{ border: 0, padding: 0, margin: 0, minWidth: 0, display: "contents" }}>
        <fieldset className="pay-options">
          <legend>{t("Amount")}</legend>
          {category && option("aside", t("What's set aside"), t("In the {category} payment category", { category: category.name }), aside)}
          {option("balance", t("The whole balance"), t("Pays the card off"), owed)}
          <label className={`pay-option${choice === "other" ? " chosen" : ""}`}>
            <input type="radio" name="pay-amount" checked={choice === "other"} onChange={() => setChoice("other")} />
            <span>
              <strong>{t("Another amount")}</strong>
            </span>
            <input
              aria-label={t("Payment amount")}
              className="num-input"
              inputMode="decimal"
              placeholder="0.00"
              value={other}
              onFocus={() => setChoice("other")}
              onChange={(e) => {
                setOther(e.target.value);
                setChoice("other");
              }}
            />
          </label>
        </fieldset>
        <div className="form-grid">
          <label>
            {t("From")}
            <select value={from} onChange={(e) => setFrom(e.target.value)}>
              {sources.map((a) => (
                <option key={a.id} value={a.id}>
                  {t("{name} ({amount})", { name: a.name, amount: money(balanceOf(a)) })}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t("Date")}
            <DateInput value={date} onChange={setDate} />
          </label>
        </div>
        <label>
          {t("Memo")}
          <input placeholder={t("Optional")} value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        </fieldset>
        <p className="help">
          {t("Recorded as a transfer from {from} to {card}, so it isn't spending: it uses up what's set aside and lowers what the card owes.", {
            from: source?.name ?? t("the account you choose"),
            card: card.name,
          })}
        </p>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {retrySave ? t("Close (draft kept)") : t("Cancel")}
          </button>
          <button className="primary" disabled={busy || !sources.length}>
            <CreditCard size={15} aria-hidden /> {retrySave ? t("Retry saving") : t("Pay {amount}", { amount: money(shown) })}
          </button>
        </div>
      </form>
    </Modal>
  );
}
