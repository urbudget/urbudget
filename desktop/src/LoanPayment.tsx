import { useState } from "react";
import { HandCoins } from "lucide-react";
import { Modal } from "./forms";
import { CategoryPicker, DateInput } from "./ui";
import { editableAmount, errorText, evaluateAmount, money } from "./format";
import { owedOn } from "./Planning";
import { accountType } from "./accounts";
import { AppliedChangeError } from "./useBudget";
import { id, today, type Account, type Budget, type LoanTerms, type Transaction } from "./types";
import { t } from "./i18n";

/**
 * Recording a loan payment: a transfer from a budget account to the loan, spending its payment
 * category (loans.rs's record). The month's interest and fees go onto the loan with its first
 * payment that month.
 */
export function LoanPaymentForm({
  budget,
  loan,
  terms,
  balances,
  close,
  record,
}: {
  budget: Budget;
  loan: Account;
  terms: LoanTerms;
  /** Working balances by account id. */
  balances: Map<string, { balance: number }>;
  close: () => void;
  record: (payment: { terms: LoanTerms; from: string; amount: number; date: string; id: string }) => Promise<void>;
}) {
  // Stable across a retry, so a save that failed late records one payment, not two.
  const [paymentId] = useState(id);
  const sources = budget.accounts.filter((a) => a.kind === "cash" && !a.archived && a.onBudget !== false);
  const balanceOf = (a: Account) => balances.get(a.id)?.balance ?? 0;
  // The account that paid this loan last; else checking, then the most in it.
  const [from, setFrom] = useState(() => {
    const last = budget.transactions
      .filter((t) => t.transferAccountId === loan.id && t.amount < 0 && sources.some((a) => a.id === t.accountId))
      .reduce<Transaction | null>((latest, t) => (!latest || t.date > latest.date ? t : latest), null);
    if (last) return last.accountId;
    const rank = (a: Account) => (accountType(a) === "checking" ? 0 : 1);
    return [...sources].sort((a, b) => rank(a) - rank(b) || balanceOf(b) - balanceOf(a))[0]?.id ?? "";
  });
  const [amount, setAmount] = useState(editableAmount(terms.monthlyPayment));
  const [date, setDate] = useState(today());
  const [category, setCategory] = useState(
    budget.categories.some((c) => c.id === terms.categoryId) ? terms.categoryId : "",
  );
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [retrySave, setRetrySave] = useState<(() => Promise<void>) | null>(null);
  let shown = 0;
  try {
    shown = evaluateAmount(amount);
  } catch {
    // Shown as 0.00 until the amount reads as money.
  }
  const source = sources.find((a) => a.id === from);
  const owed = owedOn(balances.get(loan.id)?.balance ?? loan.openingBalance);
  const charges = !terms.monthlyFees
    ? t("The month's interest is added to the loan with its first payment that month.")
    : accountType(loan) === "mortgage"
      ? t("The month's interest and escrow are added to the loan with its first payment that month.")
      : t("The month's interest and fees are added to the loan with its first payment that month.");
  return (
    <Modal title={t("Pay {name}", { name: loan.name })} close={close}>
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
            const value = evaluateAmount(amount);
            if (!source) throw new Error(t("Choose the account the payment comes from."));
            if (value <= 0) throw new Error(t("Enter an amount to pay."));
            if (!category) throw new Error(t("Choose the category the payment comes from."));
            if (!date) throw new Error(t("Choose a date."));
            setBusy(true);
            setError("");
            await record({ terms: { ...terms, categoryId: category }, from: source.id, amount: value, date, id: paymentId });
          } catch (err) {
            if (err instanceof AppliedChangeError) setRetrySave(() => err.retrySave);
            setError(errorText(err));
            setBusy(false);
          }
        }}
      >
        <div className="pay-facts">
          <div>
            <span>{t("Owed on the loan")}</span>
            <strong className="num">{money(owed)}</strong>
          </div>
          <div>
            <span>{t("Monthly payment")}</span>
            <strong className="num">{money(terms.monthlyPayment)}</strong>
          </div>
        </div>
        <fieldset disabled={busy || !!retrySave} style={{ border: 0, padding: 0, margin: 0, minWidth: 0, display: "contents" }}>
        <div className="form-grid">
          <label>
            {t("Amount")}
            <input
              aria-label={t("Payment amount")}
              inputMode="decimal"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </label>
          <label>
            {t("Date")}
            <DateInput value={date} onChange={setDate} />
          </label>
        </div>
        <div className="form-grid">
          <label>
            {t("From")}
            <select value={from} onChange={(e) => setFrom(e.target.value)}>
              {sources.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name} ({money(balanceOf(a))})
                </option>
              ))}
            </select>
          </label>
          <div className="field">
            <span>{t("Category")}</span>
            <CategoryPicker
              budget={budget}
              label={t("Payment category")}
              value={category || null}
              onChange={setCategory}
              placeholder={t("Where the payment comes from")}
            />
          </div>
        </div>
        </fieldset>
        <p className="help">
          {t("Recorded as a transfer from {from} to {loan}, spending the category.", {
            from: source?.name ?? t("the account you choose"),
            loan: loan.name,
          })}{" "}
          {charges}
        </p>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {retrySave ? t("Close (draft kept)") : t("Cancel")}
          </button>
          <button className="primary" disabled={busy || !sources.length}>
            <HandCoins size={15} aria-hidden /> {retrySave ? t("Retry saving") : t("Pay {amount}", { amount: money(shown) })}
          </button>
        </div>
      </form>
    </Modal>
  );
}
