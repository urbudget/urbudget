import { useEffect, useState } from "react";
import { FileSpreadsheet, Sparkles } from "lucide-react";
import { statementFile } from "./BankImport";
import { Modal } from "./forms";
import { errorText, evaluateAmount, formatDate, money } from "./format";
import { holdings, plain, type PositionsFile } from "./holdings";
import { t, tn } from "./i18n";
import { today, type Account, type Budget } from "./types";
import { DateInput } from "./ui";

type Transform = (work: (b: Budget) => Promise<Budget>) => Promise<void>;

/** A plain decimal ("1234.5") in hundredths. */
const cents = (text: string) => Math.round(Number(text || 0) * 100);

/**
 * What a positions file says an account holds, looked over before it's kept: each holding with
 * its value and cost, the day the figures are from, and the currency they're in. A file that
 * can't be read, or that left rows out, can go to the assistant, which reads other layouts and
 * proposes the same thing.
 */
export function PositionsFileDialog({
  budget,
  account,
  file,
  transform,
  close,
  onDone,
  ask,
}: {
  budget: Budget;
  account: Account;
  file: File;
  transform: Transform;
  close: () => void;
  onDone: () => void;
  /** Hands the file to the assistant to read. */
  ask?: (file: File) => void;
}) {
  const known = budget.investments?.find((h) => h.accountId === account.id);
  const [read, setRead] = useState<PositionsFile | null>(null);
  const [error, setError] = useState("");
  const [date, setDate] = useState(today());
  const [currency, setCurrency] = useState(budget.currency);
  const [rate, setRate] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const found = await holdings.file((await statementFile(file)).text);
        if (!live) return;
        setRead(found);
        if (found.asOf) setDate(found.asOf);
        if (found.currency) setCurrency(found.currency);
      } catch (e) {
        if (live) setError(errorText(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [file]);
  const code = currency.trim().toUpperCase();
  const foreign = code !== "" && code !== budget.currency;
  // A rate given for this account before is used again unless another is typed.
  const remembered = foreign && known?.currency === code ? (known.manualRate ?? "") : "";
  const figure = (text: string) => money(cents(text), { currency: code || null });
  const toAssistant = ask && (
    <button
      type="button"
      className="secondary"
      onClick={() => {
        ask(file);
        close();
      }}
    >
      <Sparkles size={15} aria-hidden /> {t("Ask the assistant to read it")}
    </button>
  );
  return (
    <Modal title={t("Positions file · {account}", { account: account.name })} close={close} wide>
      <form
        className="holdings-setup"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!read) return;
          setBusy(true);
          setError("");
          try {
            await transform((b) =>
              holdings.record(b, account.id, {
                source: "file",
                sourceAccount: file.name,
                asOf: date,
                currency: foreign ? code : "",
                rate: foreign ? rate.trim() || remembered : "",
                positions: read.positions,
                cash: read.cash,
              }),
            );
            onDone();
          } catch (err) {
            setError(errorText(err));
            setBusy(false);
          }
        }}
      >
        {!read && !error && <p className="help">{t("Reading the file…")}</p>}
        {!read && error && (
          <>
            <p className="form-error">{error}</p>
            {ask && <p className="help">{t("The assistant can read other layouts: it looks at the file and proposes what the account holds for you to check.")}</p>}
            <div className="modal-actions">
              <button type="button" className="secondary" onClick={close}>
                {t("Cancel")}
              </button>
              {toAssistant}
            </div>
          </>
        )}
        {read && (
          <>
            <p className="import-lead">
              <FileSpreadsheet size={16} aria-hidden />{" "}
              {tn(read.positions.length, "{count} holding worth {value} in all.", "{count} holdings worth {value} in all.", { value: figure(read.value) })}
              {read.cash && ` ${t("{amount} of it is cash.", { amount: figure(read.cash) })}`}
            </p>
            <div className="table-scroll holdings-setup-rows">
              <table className="holdings-table">
                <thead>
                  <tr>
                    <th>{t("Holding")}</th>
                    <th className="num">{t("Cost")}</th>
                    <th className="num">{t("Value")}</th>
                  </tr>
                </thead>
                <tbody>
                  {read.positions.map((p, i) => (
                    <tr key={`${p.symbol}:${p.name}:${i}`}>
                      <td>
                        <span className="holdings-name">{p.name || p.symbol}</span>
                        <small>
                          <span dir="ltr">
                            {p.name && p.symbol && p.symbol.length <= 12 ? `${p.symbol} · ` : ""}
                            {p.quantity && p.price ? `${plain(p.quantity)} × ${plain(p.price)}` : p.quantity ? plain(p.quantity) : ""}
                          </span>
                        </small>
                      </td>
                      <td className="num">{p.cost ? figure(p.cost) : ""}</td>
                      <td className="num">{figure(p.value)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            {read.skipped > 0 && (
              <p className="notice">
                {tn(read.skipped, "{count} row had no value UrBudget could read and is left out.", "{count} rows had no value UrBudget could read and are left out.")}
              </p>
            )}
            <div className="form-grid">
              <label>
                {t("Prices from")}
                <DateInput value={date} onChange={setDate} />
              </label>
              <label>
                {t("Currency")}
                <input value={currency} maxLength={3} spellCheck={false} autoComplete="off" onChange={(e) => setCurrency(e.target.value)} />
              </label>
              {foreign && (
                <label>
                  {t("1 {from} in {to}", { from: code, to: budget.currency })}
                  <input inputMode="decimal" value={rate} placeholder={remembered} onChange={(e) => setRate(e.target.value)} />
                </label>
              )}
            </div>
            <p className="help">
              {t("The account's balance becomes this value. From here on, what it changes by beyond the money you put in or take out is recorded as Market change.")}
            </p>
            {error && <p className="form-error">{error}</p>}
            <div className="modal-actions">
              <button type="button" className="secondary" onClick={close}>
                {t("Cancel")}
              </button>
              {read.skipped > 0 && toAssistant}
              <button className="primary" disabled={busy || (foreign && !(rate.trim() || remembered))}>
                {busy ? t("Saving…") : t("Save holdings")}
              </button>
            </div>
          </>
        )}
      </form>
    </Modal>
  );
}

/**
 * The account's value on a day, typed in: for a broker with nothing to download. Its balance
 * follows the value, and a day in the past fills its history in.
 */
export function TypedValueDialog({
  budget,
  account,
  transform,
  close,
  onDone,
}: {
  budget: Budget;
  account: Account;
  transform: Transform;
  close: () => void;
  onDone: () => void;
}) {
  const known = budget.investments?.find((h) => h.accountId === account.id);
  const [value, setValue] = useState("");
  const [date, setDate] = useState(today());
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={t("Value of {account}", { account: account.name })} close={close}>
      <form
        className="holdings-setup"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError("");
          try {
            const amount = evaluateAmount(value);
            await transform((b) => holdings.record(b, account.id, { source: "manual", asOf: date, total: (amount / 100).toFixed(2) }));
            onDone();
          } catch (err) {
            setError(errorText(err));
            setBusy(false);
          }
        }}
      >
        <p className="import-lead">
          {t("Type what the account is worth on a day, as your broker shows it. Do it again whenever you like: once a month is plenty.")}
        </p>
        <div className="form-grid">
          <label>
            {t("Value")}
            <input className="num-input" inputMode="decimal" placeholder="0.00" autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
          </label>
          <label>
            {t("On")}
            <DateInput value={date} onChange={setDate} />
          </label>
        </div>
        {known && known.values.length > 0 && (
          <p className="help">
            {t("Last value: {amount} on {date}.", { amount: money(known.total), date: formatDate(known.asOf) })}{" "}
            {t("An earlier day adds to its history.")}
          </p>
        )}
        <p className="help">
          {t("The account's balance becomes this value. From here on, what it changes by beyond the money you put in or take out is recorded as Market change.")}
        </p>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary" disabled={busy || !value.trim()}>
            {busy ? t("Saving…") : t("Save value")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
