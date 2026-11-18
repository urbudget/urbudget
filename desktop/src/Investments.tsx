import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  Banknote,
  ChartSpline,
  Check,
  CircleDot,
  Clock,
  Coins,
  FileText,
  FileUp,
  Gem,
  Landmark,
  Layers,
  LoaderCircle,
  RefreshCw,
  TrendingUp,
  X,
  type LucideIcon,
} from "lucide-react";
import { ImportReview } from "./BankImport";
import { api } from "./bridge";
import { accountType, isLoanType } from "./accounts";
import { Modal } from "./forms";
import { PositionsFileDialog, TypedValueDialog } from "./HoldingsSetup";
import { kindName, pickPositionsFile, plain, sourceName, whatItIs } from "./holdings";
import { ColumnChart, Donut, Legend, LinesChart } from "./charts/Charts";
import { activeFormat, errorText, formatDate, money, monthName, percent, tone } from "./format";
import { ibkr, type IbkrLink, type IbkrReview, type IbkrReviewAccount, type IbkrStatus } from "./ibkr";
import type { Tile } from "./reports/shared";
import { id, today, type Account, type Budget, type Holdings, type HoldingsPage } from "./types";
import { Tabs, useToast } from "./ui";
import { daysAgo } from "./BankSync";
import { monthNames, t, tn } from "./i18n";
import "./import.css";
import "./reports.css";
import "./investments.css";

type Transform = (work: (b: Budget) => Promise<Budget>) => Promise<void>;
/**
 * What to ask Client Portal's "Configure Query with AI" for, in its own words: the sections the
 * reader uses, each with all of its fields. It takes at most 200 characters.
 */
export const FLEX_REQUEST =
  "Activity Flex Query, XML, Last 365 Calendar Days, Currency Rates Yes. All fields of: Account Information, Cash Report, Cash Transactions, NAV in Base, Open Positions (Summary), Trades, Transfers.";
/** What an investment account's header needs from the app. App provides them. */
export type InvestmentActions = {
  transform: Transform;
  /** Opens Settings › Import, where the connection is. */
  settings: () => void;
  /** Opens the assistant with a request ready to send, and files attached to it. */
  ask?: (request: { text: string; files?: File[] }) => void;
};
export const InvestmentActionsContext = createContext<InvestmentActions | null>(null);

/** An account's holdings, when a broker fills it. */
export const holdingsOf = (b: Pick<Budget, "investments">, accountId: string) =>
  b.investments?.find((h) => h.accountId === accountId);
/** Holdings whose prices are more than four days old: past a weekend, the broker has newer ones. */
export const staleHoldings = (h: Pick<Holdings, "asOf">, now = today()) => h.asOf < daysAgo(4, now);
export { plain };

/**
 * Fetches the statement (unless one is at hand) and brings it into the budget. A statement that
 * says what the last one did isn't a change: nothing is saved, and there's nothing to undo.
 */
async function sync(budget: Budget, transform: Transform, links: IbkrLink[], fetch: boolean): Promise<IbkrReview> {
  if (fetch) await ibkr.fetch();
  const ahead = await ibkr.apply(budget, links);
  if (ahead === budget) return ibkr.review(budget);
  let review: IbkrReview | undefined;
  await transform(async (b) => {
    // The budget the change was worked out on, unless something else changed it meanwhile.
    const next = b === budget ? ahead : await ibkr.apply(b, links);
    review = await ibkr.review(next);
    return next;
  });
  return review!;
}
const fresh = (a?: IbkrReviewAccount) => a?.rows.filter((r) => !r.duplicate).length ?? 0;

/** Later than any day: an account with no transactions has no history a deposit can come from. */
const NEVER = "9999-12-31";
/**
 * The accounts a deposit can have come from or a withdrawal gone to, the ones this account
 * already has the most transfers with first, each with the day its history starts.
 */
function otherAccounts(budget: Budget, account: Account) {
  const moved = new Map<string, number>();
  // Each account's first transaction, on either side of a transfer: a deposit from before it
  // is already in its opening balance, so it isn't offered as where that came from.
  const since = new Map<string, string>();
  const seen = (id: string | null | undefined, date: string) => {
    if (id && date < (since.get(id) ?? NEVER)) since.set(id, date);
  };
  for (const x of budget.transactions) {
    const other = x.accountId === account.id ? x.transferAccountId : x.transferAccountId === account.id ? x.accountId : null;
    if (other) moved.set(other, (moved.get(other) ?? 0) + 1);
    seen(x.accountId, x.date);
    seen(x.transferAccountId, x.date);
    for (const s of x.splits) seen(s.transferAccountId, x.date);
  }
  return budget.accounts
    .filter((a) => a.id !== account.id && !a.archived)
    .map((a, i) => ({ id: a.id, name: a.name, since: since.get(a.id) ?? NEVER, rank: -(moved.get(a.id) ?? 0), i }))
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map(({ id, name, since }) => ({ id, name, since }));
}

/** The deposits, withdrawals and transfers the broker listed, reviewed like a bank's rows. */
function Deposits({
  budget,
  account,
  review,
  transform,
  close,
  onDone,
}: {
  budget: Budget;
  account: Account;
  review: IbkrReviewAccount;
  transform: Transform;
  close: () => void;
  onDone: (count: number) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  return (
    <Modal title={t("Deposits and transfers · {account}", { account: account.name })} close={close} wide>
      <div className="bank-import">
        <p className="import-lead">
          {t("Money you moved into or out of Interactive Brokers. Each one is matched to the transfer already in your budget, joined to its other side in the account it came from, or added as a transfer from the account you choose. Either way it counts as money you put in, not as a gain.")}
        </p>
        <ImportReview
          budget={budget}
          rows={review.rows}
          others={otherAccounts(budget, account)}
          plainLabel={t("Add with no other account")}
          busy={busy}
          icon={<Landmark size={18} aria-hidden />}
          onCancel={close}
          onImport={async (choices, count) => {
            setBusy(true);
            setMessage("");
            try {
              await transform((b) => ibkr.import(b, account.id, choices));
              onDone(count);
            } catch (e) {
              setMessage(errorText(e));
            } finally {
              setBusy(false);
            }
          }}
        />
        {message && (
          <p role="status" className="import-result">
            {message}
          </p>
        )}
      </div>
    </Modal>
  );
}

/** Interactive Brokers' part of Settings › Import › Investments: connect it and choose the account each of its accounts fills. */
export function IbkrSettings({ budget, transform }: { budget: Budget; transform: Transform }) {
  const toast = useToast();
  const [status, setStatus] = useState<IbkrStatus | null>(null);
  const [review, setReview] = useState<IbkrReview | null>(null);
  const [token, setToken] = useState("");
  const [query, setQuery] = useState("");
  // Where each new Interactive Brokers account goes: "new", a tracking account's id, or "" for nowhere.
  const [into, setInto] = useState<Record<string, string>>({});
  const [rates, setRates] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  // Being asked about before it goes: one login, by its query id, or (true) the whole connection.
  const [removing, setRemoving] = useState<string | boolean>(false);
  const [reviewing, setReviewing] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [daily, setDaily] = useState(dailySync.on);
  const file = useRef<HTMLInputElement>(null);
  const latest = useRef(budget);
  latest.current = budget;
  const show = async (s: IbkrStatus) => {
    setStatus(s);
    setReview(s.accounts.length ? await ibkr.review(latest.current) : null);
  };
  useEffect(() => {
    ibkr.status().then(show, () => setStatus(null));
  }, []);
  const run = async (what: string, work: () => Promise<void>) => {
    setBusy(what);
    setMessage("");
    try {
      await work();
      return true;
    } catch (e) {
      setMessage(errorText(e));
      return false;
    } finally {
      setBusy("");
    }
  };
  if (!status) return null;
  // The statement's accounts. Ones the budget only keeps (their login isn't here) are listed
  // after them, as what they are.
  const accounts = (review?.accounts ?? []).filter((a) => !a.kept);
  const name = (accountId: string | null) => budget.accounts.find((a) => a.id === accountId)?.name ?? "";
  const target = (a: IbkrReviewAccount) => into[a.id] ?? "new";
  const rate = (a: IbkrReviewAccount) => (rates[a.id] ?? "").trim().replace(",", ".");
  const rated = (a: IbkrReviewAccount) => !a.needsRate || Number(rate(a)) > 0;
  const adding = accounts.filter((a) => !a.accountId && target(a));
  // Accounts a tracking account could take: not a loan, and not already filled by a broker.
  const open = budget.accounts.filter(
    (a) => a.kind === "tracking" && !a.archived && !isLoanType(accountType(a)) && !holdingsOf(budget, a.id),
  );
  const links = (): IbkrLink[] => [
    ...adding.map((a) => ({
      source: a.id,
      account: target(a) === "new" ? id() : target(a),
      create: target(a) === "new",
      rate: a.needsRate ? rate(a) : undefined,
    })),
    // A linked account waiting for its exchange rate.
    ...accounts
      .filter((a) => a.accountId && a.needsRate && Number(rate(a)) > 0)
      .map((a) => ({ source: a.id, account: a.accountId!, rate: rate(a) })),
  ];
  const bring = (fetch: boolean) =>
    run("sync", async () => {
      const count = adding.length;
      const next = await sync(latest.current, transform, links(), fetch);
      setReview(next);
      const latestStatus = await ibkr.status();
      setStatus(latestStatus);
      const failed = next.accounts.find((a) => a.accountId && a.error);
      if (failed) setMessage(failed.error!);
      // A login that couldn't be fetched says why in the list of logins.
      else if (!latestStatus.logins.some((l) => l.error))
        toast({
          message: count
            ? tn(count, "Added {count} account from Interactive Brokers.", "Added {count} accounts from Interactive Brokers.")
            : t("Your Interactive Brokers accounts are up to date."),
        });
    });
  const ready = adding.every(rated);
  // What this budget already holds from Interactive Brokers, whichever device brought it in.
  const linked = (budget.investments ?? []).filter((h) => h.source === "ibkr" && budget.accounts.some((a) => a.id === h.accountId));
  const unlink = async (accountId: string) => {
    await transform(async (b) => ({ ...b, investments: b.investments?.filter((h) => h.accountId !== accountId) }));
    if (review) setReview(await ibkr.review(latest.current));
  };
  const reviewed = accounts.find((a) => a.accountId === reviewing);
  // The two things to paste.
  const fields = (
    <div className="ibkr-fields">
      <label>
        {t("Query ID")}
        <input value={query} inputMode="numeric" spellCheck={false} autoComplete="off" onChange={(e) => setQuery(e.target.value)} />
      </label>
      <label>
        {t("Token")}
        <input value={token} type="password" spellCheck={false} autoComplete="off" onChange={(e) => setToken(e.target.value)} />
      </label>
    </div>
  );
  // What to do in Client Portal, step by step, with the two things to paste at the step they
  // come from; the statement file beside Connect unless the panel already offers one.
  const connect = (withFile: boolean) => (
    <>
      <ol className="ibkr-steps">
        <li>
          <strong>{t("Make a Flex Query")}</strong>
          <p>
            {t("In Client Portal, open Performance & Reports › Flex Queries and add an Activity Flex Query. If the page offers Configure Query with AI, paste this request and save what it builds:")}
          </p>
          <div className="ibkr-request">
            <code dir="ltr">{FLEX_REQUEST}</code>
            <button
              type="button"
              className="secondary small"
              onClick={() =>
                void navigator.clipboard.writeText(FLEX_REQUEST).then(
                  () => setCopied(true),
                  () => setMessage(t("Couldn't copy. Select the request and copy it yourself.")),
                )
              }
            >
              {copied ? t("Copied") : t("Copy")}
            </button>
          </div>
          <details className="ibkr-help">
            <summary>{t("Or set it up by hand")}</summary>
            <ol className="ibkr-list">
              <li>{t("Give the query a name, such as UrBudget.")}</li>
              <li>
                {t("Under Sections, tick each of these and choose Select All in it:")}
                {/* Client Portal's own names, in English whatever the interface. */}
                <ul className="ibkr-sections">
                  <li dir="ltr">Account Information</li>
                  <li dir="ltr">Cash Report</li>
                  <li dir="ltr">Cash Transactions</li>
                  <li dir="ltr">Net Asset Value (NAV) in Base</li>
                  <li>{t("Open Positions, choosing Summary")}</li>
                  <li>{t("Trades, choosing Executions")}</li>
                  <li dir="ltr">Transfers</li>
                </ul>
              </li>
              <li>{t("Under Delivery Configuration, set Format to XML and Period to Last 365 Calendar Days.")}</li>
              <li>{t("Under General Configuration, set Include Currency Rates to Yes.")}</li>
              <li>{t("Save it with Continue, then Create.")}</li>
            </ol>
          </details>
        </li>
        <li>
          <strong>{t("Make a token")}</strong>
          <ul className="ibkr-list">
            <li>{t("On the same page, open Flex Web Service Configuration and turn the service on.")}</li>
            <li>{t("Generate a token. Choose one year so it lasts, and leave the IP address restriction empty.")}</li>
          </ul>
        </li>
        <li>
          <strong>{t("Paste both here")}</strong>
          <ul className="ibkr-list">
            <li>{t("Query ID: on the Flex Queries page, the info (i) icon beside your query opens a box with the Query ID at the top.")}</li>
            <li>{t("Token: the one you just generated.")}</li>
          </ul>
          {fields}
        </li>
      </ol>
      <div className="form-actions">
        <span className="help">
          {status.store
            ? t("UrBudget keeps the token in {store}, never in the budget file.", { store: status.store })
            : t("This computer has no credential store to keep a token in. You can still bring in a statement file.")}
        </span>
        {withFile && (
          <button type="button" className="text-button" disabled={!!busy} onClick={() => file.current?.click()}>
            {t("Use a statement file")}
          </button>
        )}
        <button
          className="primary"
          disabled={!token.trim() || !query.trim() || !status.store || !!busy}
          onClick={async () => {
            if (await run("connect", async () => show(await ibkr.connect(token, query)))) {
              setToken("");
              setQuery("");
            }
          }}
        >
          {busy === "connect" ? t("Connecting…") : t("Connect")}
        </button>
      </div>
      {busy === "connect" && (
        <p className="bank-sync-wait">
          <LoaderCircle size={16} className="spin" aria-hidden /> {t("Interactive Brokers is preparing your statement. This can take a minute.")}
        </p>
      )}
      <details className="ibkr-help">
        <summary>{t("How it works")}</summary>
        <ul className="ibkr-how">
          <li>{t("The token can only read statements. It can't trade or move money, and it stays on this computer.")}</li>
          <li>{t("A sync asks Interactive Brokers for your query's statement. It makes each day's statement overnight, so prices are as of the last close.")}</li>
          <li>{t("UrBudget syncs when you press Sync, and once a day when it opens.")}</li>
          <li>{t("The account's balance follows Interactive Brokers' value, so your net worth and reports include it. Each month's change is added to the account as Market change.")}</li>
        </ul>
        <button type="button" className="link-button" onClick={() => void ibkr.openGuide().catch((e) => setMessage(errorText(e)))}>
          {t("Interactive Brokers' guide")}
        </button>
      </details>
    </>
  );
  // What the statement brought, so a query set up short of the list shows where.
  const first = accounts[0];
  const includes: [string, boolean][] = first
    ? [
        [t("Holdings"), !first.missing.includes("Open Positions")],
        [t("Cash"), !first.missing.includes("Cash Report")],
        [t("Daily values"), !first.missing.includes("Net Asset Value (NAV) in Base")],
        [t("Deposits, income and fees"), !first.missing.includes("Cash Transactions")],
        [t("Transfers"), !first.missing.includes("Transfers")],
        [t("Trades"), !first.missing.includes("Trades")],
        [t("Currency rates"), first.rates],
      ]
    : [];
  return (
    <div className="bank-sync ibkr">
      {!status.connected && !accounts.length && !linked.length ? (
        <>
          <p className="import-lead">
            {t("See what your Interactive Brokers account holds and what it's worth, as part of your net worth. UrBudget reads your statements straight from Interactive Brokers with a token that can only read. It can't trade or move money.")}
          </p>
          {connect(true)}
        </>
      ) : (
        <>
          <p className="import-lead">
            {status.connected
              ? t("Connected. Interactive Brokers makes each day's statement overnight, so a sync brings holdings and prices as of the last close.")
              : accounts.length
                ? t("From a statement file. Connect with a token to update without choosing a file each time.")
                : t("These accounts were filled from Interactive Brokers, but this computer has no token for it. Connect here to sync them, or bring in a statement file.")}
          </p>
          {accounts.length > 0 && (
            <div className="table-scroll">
              <table className="bank-sync-accounts">
                <thead>
                  <tr>
                    <th>{t("At Interactive Brokers")}</th>
                    <th className="num">{t("Value")}</th>
                    <th>{t("Goes into")}</th>
                  </tr>
                </thead>
                <tbody>
                  {accounts.map((a) => (
                    <tr key={a.id}>
                      <td>
                        {a.alias || a.id}
                        <small className="bank-text">
                          {a.alias && `${a.id} · `}
                          {tn(a.positions, "{count} holding", "{count} holdings")}
                          {a.asOf && ` · ${formatDate(a.asOf)}`}
                        </small>
                      </td>
                      {/* Once it's in the budget, its value there; before, in the account's own currency. */}
                      <td className="num">
                        {holdingsOf(budget, a.accountId ?? "") ? money(holdingsOf(budget, a.accountId!)!.total) : money(a.value, { currency: a.currency || null })}
                      </td>
                      <td>
                        {a.accountId ? (
                          <div className="bank-sync-since">
                            <span>{name(a.accountId)}</span>
                            {fresh(a) > 0 && (
                              <button className="secondary small" onClick={() => setReviewing(a.accountId)}>
                                {tn(fresh(a), "Review {count} deposit", "Review {count} deposits")}
                              </button>
                            )}
                            <button className="text-button" onClick={() => void run("unlink", () => unlink(a.accountId!))}>
                              {t("Unlink")}
                            </button>
                          </div>
                        ) : (
                          <select
                            aria-label={t("Account for {name}", { name: a.alias || a.id })}
                            value={target(a)}
                            onChange={(e) => setInto({ ...into, [a.id]: e.target.value })}
                          >
                            <option value="new">{t("A new investment account")}</option>
                            {open.map((o) => (
                              <option key={o.id} value={o.id}>
                                {o.name}
                              </option>
                            ))}
                            <option value="">{t("Don't add")}</option>
                          </select>
                        )}
                        {a.needsRate && (a.accountId || target(a)) && (
                          <label className="ibkr-rate">
                            {t("1 {from} in {to}", { from: a.needsRate[0], to: a.needsRate[1] })}
                            <input
                              inputMode="decimal"
                              value={rates[a.id] ?? ""}
                              onChange={(e) => setRates({ ...rates, [a.id]: e.target.value })}
                            />
                          </label>
                        )}
                      </td>
                    </tr>
                  ))}
                  {/* Filled from Interactive Brokers by a login that isn't among these: still
                      listed, so it can be seen and unlinked. */}
                  {linked
                    .filter((h) => !accounts.some((a) => a.accountId === h.accountId))
                    .map((h) => (
                      <tr key={h.accountId}>
                        <td>
                          {h.sourceAccount}
                          <small className="bank-text">{t("Its login isn't on this computer, so it doesn't update here.")}</small>
                        </td>
                        <td className="num">{money(h.total)}</td>
                        <td>
                          <div className="bank-sync-since">
                            <span>{name(h.accountId)}</span>
                            <button className="text-button" onClick={() => void run("unlink", () => unlink(h.accountId))}>
                              {t("Unlink")}
                            </button>
                          </div>
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          )}
          {/* Before a sync this session, what the budget already holds from Interactive Brokers. */}
          {!accounts.length && linked.length > 0 && (
            <div className="table-scroll">
              <table className="bank-sync-accounts">
                <thead>
                  <tr>
                    <th>{t("At Interactive Brokers")}</th>
                    <th className="num">{t("Value")}</th>
                    <th>{t("Goes into")}</th>
                  </tr>
                </thead>
                <tbody>
                  {linked.map((h) => (
                    <tr key={h.accountId}>
                      <td>
                        {h.sourceAccount}
                        <small className="bank-text">
                          {tn(h.positions.length, "{count} holding", "{count} holdings")} · {formatDate(h.asOf)}
                        </small>
                      </td>
                      <td className="num">{money(h.total)}</td>
                      <td>
                        <div className="bank-sync-since">
                          <span>{name(h.accountId)}</span>
                          <button className="text-button" onClick={() => void run("unlink", () => unlink(h.accountId))}>
                            {t("Unlink")}
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {busy === "sync" && status.connected && !adding.length && (
            <p className="bank-sync-wait">
              <LoaderCircle size={16} className="spin" aria-hidden /> {t("Interactive Brokers is preparing your statement. This can take a minute.")}
            </p>
          )}
          {accounts.some((a) => a.needsRate) && (
            <p className="help">
              {t("The statement has no exchange rate for this budget's currency, so UrBudget uses the one you give for every day. Change it here when it moves.")}
            </p>
          )}
          {includes.length > 0 && (
            <ul className="ibkr-includes" aria-label={t("What the statement includes")}>
              {includes.map(([label, there]) => (
                <li key={label} className={there ? "there" : "left-out"}>
                  {there ? <Check size={13} aria-hidden /> : <X size={13} aria-hidden />} {label}
                </li>
              ))}
            </ul>
          )}
          {accounts
            .filter((a) => a.missing.length)
            .slice(0, 1)
            .map((a) => (
              <p key={a.id} className="notice">
                {t("Your Flex Query leaves out {sections}. UrBudget shows what it can; tick them in Client Portal for the whole picture.", {
                  sections: a.missing.join(", "),
                })}
              </p>
            ))}
          {first?.generated && (
            <p className="help">
              {t("Interactive Brokers made this statement on {when}, by its own clock. It can keep sending the same one for a while, so a change to your Flex Query may take time to show.", {
                when: <span dir="ltr">{first.generated}</span>,
              })}
            </p>
          )}
          {first && !first.rates && (
            <p className="notice">
              {t("Your Flex Query leaves out the currency rates. Set Include Currency Rates to Yes in Client Portal, so holdings and deposits in other currencies convert.")}
            </p>
          )}
          {/* The logins, once there is more than one or one of them needs attention. */}
          {status.connected && (status.logins.length > 1 || status.logins.some((l) => l.error)) && (
            <ul className="ibkr-logins" aria-label={t("Logins")}>
              <li className="ibkr-logins-title" aria-hidden>
                {t("Logins")}
              </li>
              {status.logins.map((l) => (
                <li key={l.query}>
                  <div>
                    <strong>{l.accounts.length ? l.accounts.map((id) => accounts.find((a) => a.id === id)?.alias || id).join(", ") : t("Query {id}", { id: l.query })}</strong>
                    {l.accounts.length > 0 && <small className="bank-text">{t("Query {id}", { id: l.query })}</small>}
                    {l.error && <p className="ibkr-login-error">{l.error}</p>}
                  </div>
                  <button className="text-button" disabled={!!busy} onClick={() => setRemoving(l.query)}>
                    {t("Remove")}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {status.connected && (
            <label className="checkbox-label ibkr-daily">
              <input
                type="checkbox"
                checked={daily}
                onChange={(e) => {
                  dailySync.set(e.target.checked);
                  setDaily(e.target.checked);
                }}
              />
              {t("Sync once a day when UrBudget opens")}
            </label>
          )}
          {removing ? (
            <div className="notice bank-sync-remove">
              <span>
                {typeof removing === "string"
                  ? t("Remove this login? UrBudget forgets its token on this computer. Its accounts keep what they hold and stop updating. To switch the token off, do that in Client Portal too.")
                  : t("Remove the connection? UrBudget forgets the token on this computer. Your accounts keep what they hold and stop updating. To switch the token off, do that in Client Portal too.")}
              </span>
              <button className="secondary small" onClick={() => setRemoving(false)}>
                {t("Keep it")}
              </button>
              <button
                className="primary small"
                disabled={!!busy}
                onClick={async () => {
                  const one = typeof removing === "string" ? removing : undefined;
                  if (await run("remove", async () => show(await ibkr.disconnect(one)))) setRemoving(false);
                }}
              >
                {typeof removing === "string" ? t("Remove login") : t("Remove connection")}
              </button>
            </div>
          ) : (
            <div className="form-actions">
              {status.connected ? (
                // With several logins each is removed on its own line above.
                status.logins.length < 2 && (
                  <button className="text-button" onClick={() => setRemoving(true)}>
                    {t("Remove connection")}
                  </button>
                )
              ) : (
                accounts.length > 0 && (
                  <button className="text-button" onClick={() => void run("remove", async () => show(await ibkr.disconnect()))}>
                    {t("Start over")}
                  </button>
                )
              )}
              <button type="button" className={status.connected || adding.length ? "text-button" : "secondary"} disabled={!!busy} onClick={() => file.current?.click()}>
                {t("Use a statement file")}
              </button>
              {adding.length ? (
                <button className="primary" disabled={!ready || !!busy} onClick={() => void bring(false)}>
                  {busy === "sync" ? t("Adding…") : t("Add to my budget")}
                </button>
              ) : (
                // Without a token there is nothing to fetch: the next statement comes from a file.
                status.connected && (
                  <button className="primary" disabled={!!busy} onClick={() => void bring(true)}>
                    <RefreshCw size={14} className={busy === "sync" ? "spin" : undefined} /> {busy === "sync" ? t("Syncing…") : t("Sync now")}
                  </button>
                )
              )}
            </div>
          )}
          {/* A token for someone without one here, or another login for someone with two. */}
          <details className="ibkr-connect">
            <summary>{status.connected ? t("Add another login") : t("Connect with a token")}</summary>
            {status.connected && (
              <p className="help">
                {t("For a second login at Interactive Brokers, sign in to Client Portal with it and make a query and a token there too. Both logins then sync together. Accounts under one login need only one query that covers them all.")}
              </p>
            )}
            {connect(false)}
          </details>
        </>
      )}
      <input
        ref={file}
        type="file"
        accept=".xml,text/xml,application/xml"
        hidden
        onChange={async (e) => {
          const chosen = e.target.files?.[0];
          e.target.value = "";
          if (chosen) await run("file", async () => show(await ibkr.file(await chosen.text())));
        }}
      />
      {message && (
        <p role="status" className="import-result">
          {message}
        </p>
      )}
      {reviewed && reviewing && (
        <Deposits
          budget={budget}
          account={budget.accounts.find((a) => a.id === reviewing)!}
          review={reviewed}
          transform={transform}
          close={() => setReviewing(null)}
          onDone={async (count) => {
            setReviewing(null);
            setReview(await ibkr.review(latest.current));
            if (count) toast({ message: tn(count, "{count} deposit is in your budget.", "{count} deposits are in your budget.") });
          }}
        />
      )}
    </div>
  );
}

/** How many holdings show before "Show more". */
const FIRST_HOLDINGS = 10;
const PROMPT_HIDDEN = "urbudget.ibkrPromptHidden";
const DAILY = "urbudget.ibkrDaily";
const DAILY_AT = "urbudget.ibkrDailyAt";
const CHART = "urbudget.holdingsChart";
const stored = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const store = (key: string, value: string) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Without storage the choice lasts until the app closes.
  }
};
const TYPED = "urbudget.typedValues";
/** Whether typing an account's value in is offered: an advanced way, asked for in Settings. */
export const typedValues = {
  on: () => stored(TYPED) === "1",
  set: (on: boolean) => store(TYPED, on ? "1" : "0"),
};
/** What the assistant is asked when a positions file is handed to it. */
const readRequest = (account: Account) => t("Read what {account} holds from the attached file and record it.", { account: account.name });

/**
 * Settings › Import: every investment account and how it's kept up to date (Interactive Brokers,
 * a bank connection, a positions file, or a value typed in), with Interactive Brokers' own
 * connection folded under them.
 */
export function InvestmentsSettings({ budget, transform }: { budget: Budget; transform: Transform }) {
  const actions = useContext(InvestmentActionsContext);
  const toast = useToast();
  const [setup, setSetup] = useState<{ account: Account; file?: File } | null>(null);
  const [typed, setTyped] = useState(typedValues.on);
  const [broker, setBroker] = useState(() => !!budget.investments?.some((h) => h.source === "ibkr"));
  const [message, setMessage] = useState("");
  // Accounts that can hold investments (an investment or retirement account, or a tracking asset
  // with no type of its own), and any other account something already fills.
  const rows = budget.accounts.filter(
    (a) => !a.archived && (holdingsOf(budget, a.id) || (a.kind === "tracking" && ["investment", "retirement", "otherAsset"].includes(accountType(a)))),
  );
  const unlink = (accountId: string) =>
    transform(async (b) => ({ ...b, investments: b.investments?.filter((h) => h.accountId !== accountId) })).catch((e) => setMessage(errorText(e)));
  const fromFile = async (account: Account) => {
    const file = await pickPositionsFile();
    if (file) setSetup({ account, file });
  };
  const done = (account: Account) => {
    setSetup(null);
    toast({ message: t("{account} is up to date.", { account: account.name }) });
  };
  const ask = actions?.ask;
  return (
    <section className="panel bank-sync investments-panel">
      <h2>{t("Investments")}</h2>
      <p className="import-lead">
        {t("See what your investment accounts hold and how they're doing. Each one is kept up to date its own way: by Interactive Brokers, by your bank connection, or from a positions file you bring in.")}
      </p>
      {rows.length > 0 && (
        <div className="table-scroll">
          <table className="bank-sync-accounts">
            <thead>
              <tr>
                <th>{t("Account")}</th>
                <th>{t("Kept up to date by")}</th>
                <th className="num">{t("Value")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((a) => {
                const held = holdingsOf(budget, a.id);
                return (
                  <tr key={a.id}>
                    <td>{a.name}</td>
                    <td>
                      {held ? (
                        <>
                          {sourceName(held)}
                          <small className="bank-text">{held.source === "ibkr" ? held.sourceAccount : formatDate(held.asOf)}</small>
                        </>
                      ) : (
                        <span className="bank-text">{t("Nothing yet")}</span>
                      )}
                    </td>
                    <td className="num">{held ? money(held.total) : ""}</td>
                    <td>
                      <div className="bank-sync-since">
                        {!held && (
                          <button className="secondary small" onClick={() => void fromFile(a)}>
                            {t("Positions file")}
                          </button>
                        )}
                        {!held && (
                          <button className="text-button" onClick={() => setBroker(true)}>
                            Interactive Brokers
                          </button>
                        )}
                        {held?.source === "file" && (
                          <button className="secondary small" onClick={() => void fromFile(a)}>
                            {t("Update from a file")}
                          </button>
                        )}
                        {held?.source === "manual" && (
                          <button className="secondary small" onClick={() => setSetup({ account: a })}>
                            {t("Update value")}
                          </button>
                        )}
                        {typed && !held && (
                          <button className="text-button" onClick={() => setSetup({ account: a })}>
                            {t("Type its value")}
                          </button>
                        )}
                        {held && held.source !== "ibkr" && (
                          <button className="text-button" onClick={() => void unlink(a.id)}>
                            {t("Unlink")}
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <details className="ibkr-help">
        <summary>{t("How an account is kept up to date")}</summary>
        <ul className="ibkr-how">
          <li>{t("Interactive Brokers syncs holdings, prices, deposits and income with a read-only token. Set it up below.")}</li>
          <li>{t("A bank connection brings an investment account's holdings with each bank sync, when its bank sends them. Link the account under Bank sync.")}</li>
          <li>{t("A positions file is what your broker lets you download: a CSV, a spreadsheet, or an OFX or QFX statement. The assistant can read other layouts.")}</li>
          <li>{t("For a broker with nothing to download, you can type the account's value yourself. Turn that on at the bottom of this panel.")}</li>
        </ul>
      </details>
      <details className="ibkr-connect investments-broker" open={broker} onToggle={(e) => setBroker(e.currentTarget.open)}>
        <summary>Interactive Brokers</summary>
        <IbkrSettings budget={budget} transform={transform} />
      </details>
      <label className="checkbox-label ibkr-daily">
        <input
          type="checkbox"
          checked={typed}
          onChange={(e) => {
            typedValues.set(e.target.checked);
            setTyped(e.target.checked);
          }}
        />
        {t("Advanced: let me type an account's value myself")}
      </label>
      {message && (
        <p role="status" className="import-result">
          {message}
        </p>
      )}
      {setup &&
        (setup.file ? (
          <PositionsFileDialog
            budget={budget}
            account={setup.account}
            file={setup.file}
            transform={transform}
            close={() => setSetup(null)}
            onDone={() => done(setup.account)}
            ask={ask && ((file) => ask({ text: readRequest(setup.account), files: [file] }))}
          />
        ) : (
          <TypedValueDialog budget={budget} account={setup.account} transform={transform} close={() => setSetup(null)} onDone={() => done(setup.account)} />
        ))}
    </section>
  );
}

/** Whether the app syncs with Interactive Brokers by itself, once a day, when it opens. */
export const dailySync = {
  on: () => stored(DAILY) !== "0",
  set: (on: boolean) => store(DAILY, on ? "1" : "0"),
  /** Not tried yet today. */
  due: (now = today()) => stored(DAILY_AT) !== now,
  tried: (now = today()) => store(DAILY_AT, now),
};

/**
 * Syncs the budget's Interactive Brokers accounts once a day, a few seconds after the app
 * opens, when this computer has the token. The broker makes one statement a day, so once is
 * enough; a failure stays quiet (the account's page says when prices are old) and isn't tried
 * again until tomorrow.
 */
export function useDailyIbkrSync(budget: Budget | null | undefined, transform: Transform) {
  const toast = useToast();
  const latest = useRef(budget);
  latest.current = budget;
  const work = useRef(transform);
  work.current = transform;
  const filled = !!budget?.investments?.some((h) => h.source === "ibkr");
  useEffect(() => {
    if (!filled || !dailySync.on() || !dailySync.due()) return;
    const timer = setTimeout(async () => {
      try {
        if (!latest.current || !dailySync.due() || !(await ibkr.status()).connected) return;
        dailySync.tried();
        const before = latest.current;
        const had = before.investments?.map((h) => h.asOf).sort().pop() ?? "";
        const review = await sync(before, (run) => work.current(run), [], true);
        const has = review.accounts.filter((a) => a.accountId).map((a) => a.asOf).sort().pop() ?? "";
        if (has > had) toast({ message: t("Interactive Brokers: prices from {date}.", { date: formatDate(has) }) });
      } catch {
        // Offline, or the statement isn't ready: tomorrow's try, or Sync, brings it.
      }
    }, 4000);
    return () => clearTimeout(timer);
  }, [filled]);
}

type Sort = { key: "name" | "value" | "gain" | "rate"; desc: boolean };
/** What the chart draws: what the account is worth, or what it has gained. */
type Chart = "value" | "gain";
/** A holding's gain as a share of what it cost, when the broker gives both. */
/** How far back the figures look, as the engine names the periods. */
const PERIODS = ["1m", "3m", "ytd", "1y", "all"] as const;
type Period = (typeof PERIODS)[number];

/**
 * The engine's figures for an account's page (investment_stats.rs `page`), read again when the
 * budget changes. While a new read is on its way the last page for the same account stays, so
 * an edit elsewhere doesn't blank the view; `on` false asks for nothing.
 */
function useHoldingsPage(budget: Budget, accountId: string, on: boolean) {
  const [result, setResult] = useState<{ accountId: string; budget: Budget; attempt: number; data?: HoldingsPage; error?: string } | null>(null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!on) return;
    let cancelled = false;
    const controller = new AbortController();
    api
      .investmentPage(budget, accountId, controller.signal)
      .then((data) => !cancelled && setResult({ accountId, budget, attempt, data: data ?? undefined, error: data ? undefined : t("No holdings for this account.") }))
      .catch((e) => !cancelled && setResult({ accountId, budget, attempt, error: errorText(e) }));
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [budget, accountId, attempt, on]);
  const mine = on && result?.accountId === accountId ? result : null;
  const current = mine?.budget === budget && mine.attempt === attempt;
  return {
    data: mine?.data,
    error: current ? mine.error : undefined,
    retry: () => setAttempt((n) => n + 1),
  };
}

/**
 * What an investment account holds and how it's doing, over its register: the gain and return
 * over a chosen period with the value's line, the day's change, what the holdings cost and
 * have gained, cash, what the account earned and was charged, and each holding. The figures
 * are the broker's; UrBudget adds them up and prices nothing itself. An investment account
 * nothing fills yet offers the connection instead.
 */
/** The reports' series colours, in order; past the eighth they share the last one. */
const seriesColour = (i: number) => (i >= 0 && i < 8 ? `var(--series-${i + 1})` : "var(--series-other)");

/** A picture for each kind of holding (keys from `kindGroup`, "Cash", "Accrued"). */
const KIND_ICONS: Record<string, LucideIcon> = {
  Stocks: TrendingUp,
  Funds: Layers,
  Bonds: Landmark,
  Options: FileText,
  Futures: FileText,
  CFDs: FileText,
  Currencies: Coins,
  Crypto: Coins,
  Commodities: Gem,
  Cash: Banknote,
  Accrued: Clock,
};
/** A holding's mark, in its kind's colour: a share's short ticker itself, else its kind's picture. */
function Badge({ group, symbol = "", colour }: { group: string; symbol?: string; colour: string }) {
  const Icon = KIND_ICONS[group] ?? CircleDot;
  const ticker = ["Stocks", "Funds", "Crypto"].includes(group) && /^[A-Z0-9.]{1,4}$/.test(symbol);
  return (
    <span className={`holdings-badge${ticker ? " ticker" : ""}`} style={{ "--kind": colour } as CSSProperties} aria-hidden>
      {ticker ? symbol : <Icon size={16} />}
    </span>
  );
}

/** Figures side by side: each one's name, its amount, and a line of detail under it. */
function Stats({ items, className = "" }: { items: Tile[]; className?: string }) {
  return (
    <dl className={`holdings-stats ${className}`.trim()}>
      {items.map((x) => (
        <div key={x.label}>
          <dt>{x.label}</dt>
          <dd className={x.tone ?? ""}>{x.value}</dd>
          {x.note && <dd className="holdings-stat-note">{x.note}</dd>}
        </div>
      ))}
    </dl>
  );
}

/** What each section a Flex Query can leave out costs this page, by the section's name in the statement. */
export const lackOf = (section: string) =>
  ({
    "Net Asset Value (NAV) in Base": t("Net Asset Value (NAV) in Base: without it the value is only known on the days you sync, so the chart and the gains start from there."),
    "Open Positions": t("Open Positions: without it the holdings aren't listed."),
    "Cash Transactions": t("Cash Transactions: without it deposits, dividends, interest and fees aren't known, and money you put in can be counted as a gain."),
    "Cash Report": t("Cash Report: without it cash and the commissions on trades aren't shown."),
    Transfers: t("Transfers: without it money moved in from another account is only estimated, from jumps in the value."),
    Trades: t("Trades: without it realised gains and commissions on trades aren't shown by month."),
  })[section];

export function HoldingsHeader({
  budget,
  account,
  balance,
  views,
}: {
  budget: Budget;
  account: Account;
  balance: number;
  /** The account page's own switch between its views, shown on the line with Sync. */
  views?: ReactNode;
}) {
  const actions = useContext(InvestmentActionsContext);
  const toast = useToast();
  const held = holdingsOf(budget, account.id);
  const [status, setStatus] = useState<IbkrStatus | null>(null);
  const [review, setReview] = useState<IbkrReviewAccount | undefined>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [reviewing, setReviewing] = useState(false);
  const [all, setAll] = useState(false);
  const [hidden, setHidden] = useState(() => stored(PROMPT_HIDDEN) === "1");
  const [period, setPeriod] = useState<Period>("1y");
  const [sort, setSort] = useState<Sort>({ key: "value", desc: true });
  const [chart, setChart] = useState<Chart>(() => (stored(CHART) === "gain" ? "gain" : "value"));
  // A positions file chosen for it, or its value being typed in.
  const [setup, setSetup] = useState<{ file: File } | "value" | null>(null);
  const latest = useRef(budget);
  latest.current = budget;
  const check = async () => {
    const next = await ibkr.review(latest.current);
    setReview(next.accounts.find((a) => a.accountId === account.id));
  };
  useEffect(() => {
    setReview(undefined);
    setMessage("");
    setAll(false);
    setStatus(null);
    // Only Interactive Brokers has a connection to ask about and deposits to review.
    if (!held || held.source !== "ibkr") return;
    ibkr.status().then(setStatus, () => setStatus(null));
    // What the last sync, on any device, left to review: the budget keeps it, so no statement
    // is needed here.
    void check().catch(() => {});
  }, [account.id, !!held, held?.syncedAt]);
  const page = useHoldingsPage(budget, account.id, !!held);
  const fromFile = async () => {
    const file = await pickPositionsFile();
    if (file) setSetup({ file });
  };
  const updated = () => {
    setSetup(null);
    toast({ message: t("{account} is up to date.", { account: account.name }) });
  };
  const ask = actions?.ask;
  const dialogs =
    actions &&
    setup &&
    (setup === "value" ? (
      <TypedValueDialog budget={budget} account={account} transform={actions.transform} close={() => setSetup(null)} onDone={updated} />
    ) : (
      <PositionsFileDialog
        budget={budget}
        account={account}
        file={setup.file}
        transform={actions.transform}
        close={() => setSetup(null)}
        onDone={updated}
        ask={ask && ((file) => ask({ text: readRequest(account), files: [file] }))}
      />
    ));
  if (!held) {
    const unfilled = account.kind === "tracking" && accountType(account) === "investment" && !account.archived;
    if (!unfilled || !actions || hidden) return null;
    return (
      <>
        <div className="loan-header loan-prompt holdings-prompt">
          <ChartSpline size={18} aria-hidden />
          <p>
            <strong>{t("See what this account holds and how it's doing")}</strong>
            <span>{t("Connect Interactive Brokers, or bring in the positions file your broker lets you download.")}</span>
          </p>
          <div className="loan-actions">
            <button
              className="text-button"
              onClick={() => {
                store(PROMPT_HIDDEN, "1");
                setHidden(true);
              }}
            >
              {t("Not now")}
            </button>
            {typedValues.on() && (
              <button className="text-button" onClick={() => setSetup("value")}>
                {t("Type its value")}
              </button>
            )}
            <button className="secondary" onClick={() => void fromFile()}>
              {t("Positions file")}
            </button>
            <button className="secondary" onClick={actions.settings}>
              Interactive Brokers
            </button>
          </div>
        </div>
        {dialogs}
      </>
    );
  }
  const stale = staleHoldings(held);
  // Something entered by hand since the last sync leaves the balance off the broker's value.
  const apart = balance - held.total;
  const syncNow = async () => {
    if (!actions) return;
    setBusy(true);
    setMessage("");
    try {
      const next = await sync(latest.current, actions.transform, [], true);
      const mine = next.accounts.find((a) => a.accountId === account.id);
      setReview(mine);
      // With several logins the others can come in while this account's own is refused.
      const now = await ibkr.status();
      setStatus(now);
      const refused = now.logins.find((l) => l.error && l.accounts.includes(held?.sourceAccount ?? ""));
      if (refused) setMessage(refused.error!);
      else if (mine?.error) setMessage(mine.error);
      else if (mine?.needsRate) setMessage(t("This account needs an exchange rate. Give it in Settings › Import."));
      else toast({ message: t("{account} is up to date.", { account: account.name }) });
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  // The account's views and what can be done with it, on one line over the cards.
  const bar = (
    <div className="holdings-bar">
      {views}
      <div className="loan-actions">
        {fresh(review) > 0 && (
          <button className="primary" onClick={() => setReviewing(true)}>
            {tn(fresh(review), "Review {count} deposit", "Review {count} deposits")}
          </button>
        )}
        {actions && held.source === "file" && (
          <button className="secondary" onClick={() => void fromFile()}>
            <FileUp size={14} aria-hidden /> {t("Update from a file")}
          </button>
        )}
        {actions && held.source === "manual" && (
          <button className="secondary" onClick={() => setSetup("value")}>
            {t("Update value")}
          </button>
        )}
        {actions &&
          (status?.connected ? (
            <button className="secondary" disabled={busy} onClick={() => void syncNow()}>
              <RefreshCw size={14} className={busy ? "spin" : undefined} /> {busy ? t("Syncing…") : t("Sync")}
            </button>
          ) : (
            status && (
              <button className="secondary" onClick={actions.settings}>
                {t("Connect on this computer")}
              </button>
            )
          ))}
      </div>
    </div>
  );
  // Where the figures come from, under the cards, and what is open over them.
  const after = (
    <>
      <p className="loan-note">
        {held.source === "ibkr"
          ? t("From Interactive Brokers account {id}, with prices from {date}. Each month's change in value is listed under Transactions as Market change.", {
              id: held.sourceAccount,
              date: formatDate(held.asOf),
            })
          : held.source === "simplefin"
            ? t("From your bank connection, as of {date}. Each month's change in value is listed under Transactions as Market change.", { date: formatDate(held.asOf) })
            : held.source === "file"
              ? t("From a positions file, with prices from {date}. Each month's change in value is listed under Transactions as Market change.", { date: formatDate(held.asOf) })
              : t("From the value you typed for {date}. Each month's change in value is listed under Transactions as Market change.", { date: formatDate(held.asOf) })}
        {stale && (held.source === "ibkr" || held.source === "simplefin") && ` ${t("These prices are a few days old.")}`}
        {(held.source === "file" || held.source === "manual") && held.asOf < daysAgo(35) && ` ${t("That is over a month ago: update it to keep the gain current.")}`}
        {apart !== 0 &&
          ` ${t("The balance is {amount} away from Interactive Brokers' value. A sync brings them back together.", { amount: money(Math.abs(apart)) })}`}
      </p>
      {message && (
        <p role="status" className="import-result">
          {message}
        </p>
      )}
      {dialogs}
      {reviewing && review && actions && (
        <Deposits
          budget={budget}
          account={account}
          review={review}
          transform={actions.transform}
          close={() => setReviewing(false)}
          onDone={(count) => {
            setReviewing(false);
            void check().catch(() => {});
            if (count) toast({ message: tn(count, "{count} deposit is in your budget.", "{count} deposits are in your budget.") });
          }}
        />
      )}
    </>
  );
  // The engine's figures for the page, read when the account or the budget changes.
  const p = page.data;
  if (!p)
    return (
      <section className="holdings-view" aria-label={t("Holdings")}>
        {bar}
        <div className="holdings-card">
          {page.error ? (
            <p className="form-error" role="alert">
              {page.error}{" "}
              <button className="link-button" onClick={page.retry}>
                {t("Try again")}
              </button>
            </p>
          ) : (
            <div className="empty-state compact">
              <LoaderCircle className="spin" />
            </div>
          )}
        </div>
        {after}
      </section>
    );
  const did = p.periods.find((x) => x.period === period) ?? null;
  // The period's days out of the whole run: its running gain is the run's, less the figure on its first day.
  const first = did ? Math.min(did.first, Math.max(0, p.days.length - 1)) : 0;
  const dates = did ? p.days.slice(first) : [];
  const values = did ? p.values.slice(first) : [];
  const gains = did ? p.gains.slice(first).map((g) => g - (p.gains[first] ?? 0)) : [];
  const day = p.dayChange;
  const sums = p.totals;
  const paid = p.earned;
  const other = p.accrued;
  // What it's made of, among the parts worth something: a short option isn't a share of it.
  const kinds = sums.byKind.filter((part) => part.value > 0);
  const currencies = sums.byCurrency.filter((part) => part.value > 0);
  // A part of the account; kept left to right, so "<1%" reads the same in Arabic.
  const share = (part: number | null) => (part === null ? null : <span dir="ltr">{percent(part)}</span>);
  const ofTotal = (value: number) => (p.total > 0 && value > 0 ? value / p.total : null);
  // A gain's sign stays in front of its amount in a right-to-left language too.
  const signed = (amount: number) => (
    <span dir="ltr">
      {amount > 0 ? "+" : ""}
      {money(amount)}
    </span>
  );
  const rate = (r: number | null) => (r === null ? null : <span dir="ltr">{percent(r, true)}</span>);
  const sorted = [...p.holdings].sort((a, b) => {
    const by =
      sort.key === "name"
        ? a.name.localeCompare(b.name)
        : sort.key === "gain"
          ? (a.gain ?? -Infinity) - (b.gain ?? -Infinity)
          : sort.key === "rate"
            ? (a.rate ?? -Infinity) - (b.rate ?? -Infinity)
            : a.value - b.value;
    return sort.desc ? -by : by;
  });
  const shown = all ? sorted : sorted.slice(0, FIRST_HOLDINGS);
  const more = sorted.length - FIRST_HOLDINGS;
  const heading = (key: Sort["key"], label: string, className = "") => (
    <th className={className} aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : undefined}>
      <button
        type="button"
        className="holdings-sort"
        onClick={() => setSort({ key, desc: sort.key === key ? !sort.desc : key !== "name" })}
      >
        {label}
        {sort.key === key && <span aria-hidden>{sort.desc ? " ↓" : " ↑"}</span>}
      </button>
    </th>
  );
  const periodName: Record<Period, string> = {
    "1m": t("1M"),
    "3m": t("3M"),
    ytd: t("YTD"),
    "1y": t("1Y"),
    all: t("All"),
  };
  const months = new Set(dates.map((d) => d.slice(0, 7))).size;
  const monthLabel = (d: string) =>
    monthNames()?.[Number(d.slice(5, 7)) - 1] ?? new Date(`${d}T12:00:00`).toLocaleDateString("en-GB", { month: "short" });
  // The chart's dates: a month or two is marked by the week (3 Nov), up to four months every
  // other week, and anything longer by the month, with the year where it turns.
  const week = (d: string) => Math.floor((Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10))) / 864e5 + 3) / 7);
  const dayTick = (d: string, i: number) => {
    if (i === 0) return null;
    const before = dates[i - 1];
    if (months <= 4) {
      if (week(d) === week(before) || (months > 2 && week(d) % 2 !== 0)) return null;
      const day = String(Number(d.slice(8, 10)));
      return activeFormat().date === "mdy" ? `${monthLabel(d)} ${day}` : `${day} ${monthLabel(d)}`;
    }
    if (before.slice(0, 7) === d.slice(0, 7) || (months > 14 && d.slice(5, 7) !== "01")) return null;
    return d.slice(5, 7) === "01" ? d.slice(0, 4) : monthLabel(d);
  };
  // A gain in a tooltip or an axis: plain text, its sign in front.
  const gainText = (amount: number) => `${amount > 0 ? "+" : ""}${money(amount)}`;
  const toned = (amount: number) => (amount < 0 ? "negative" : amount > 0 ? "positive" : undefined);
  // A share written to a tenth of a percent, for the small ones a whole percent would round away.
  const tenths = (part: number) => <span dir="ltr">{part !== 0 && Math.abs(part) < 0.0005 ? "<0.1%" : percent(Math.abs(part), true).replace("+", "")}</span>;
  // How the period went: under the chart.
  const periodStats: Tile[] = [];
  if (day) periodStats.push({ label: t("Change on {date}", { date: formatDate(held.asOf) }), value: signed(day.amount), tone: toned(day.amount) });
  if (did) periodStats.push({ label: t("Money put in"), value: signed(did.put), note: t("Since {date}", { date: formatDate(did.from) }) });
  const moves = did?.swings;
  if (moves) {
    if (moves.best) periodStats.push({ label: t("Best day"), value: signed(moves.best.amount), tone: "positive", note: formatDate(moves.best.date) });
    if (moves.worst) periodStats.push({ label: t("Worst day"), value: signed(moves.worst.amount), tone: "negative", note: formatDate(moves.worst.date) });
    if (moves.drop)
      periodStats.push({
        label: t("Largest drop"),
        value: signed(moves.drop.amount),
        tone: "negative",
        note: (
          <>
            {moves.drop.rate !== null && <>{rate(moves.drop.rate)} · </>}
            {t("{from} to {to}", { from: formatDate(moves.drop.from), to: formatDate(moves.drop.to) })}
          </>
        ),
      });
    if (moves.moved > 0)
      periodStats.push({
        label: t("Days up"),
        value: <span dir="ltr">{percent(moves.up / moves.moved)}</span>,
        note: tn(moves.moved, "{up} of {count} day", "{up} of {count} days", { up: moves.up }),
      });
  }
  // What is held: beside the ring.
  const holdStats: Tile[] = [];
  if (sums.unrealised !== null)
    holdStats.push({
      label: t("Gain on what you hold"),
      value: signed(sums.unrealised),
      tone: toned(sums.unrealised),
      note: t("{rate} on a cost of {cost}", { rate: rate(sums.unrealisedRate) ?? "—", cost: money(sums.cost ?? 0) }),
    });
  holdStats.push({
    label: t("Cash"),
    value: money(sums.cash),
    note: p.total > 0 ? t("{share} of the account", { share: share(ofTotal(sums.cash)) ?? "0%" }) : undefined,
  });
  if (p.biggest) holdStats.push({ label: t("Largest holding"), value: share(p.biggest.rate), note: p.biggest.name });
  if (p.best && p.worst) {
    holdStats.push({ label: t("Best performer"), value: rate(p.best.rate), tone: toned(p.best.rate), note: p.best.name });
    holdStats.push({ label: t("Worst performer"), value: rate(p.worst.rate), tone: toned(p.worst.rate), note: p.worst.name });
  }
  // What it earned and was charged: over their lines.
  const incomeStats: Tile[] = [];
  if (paid) {
    const income = paid.dividends + paid.interest;
    const costs = paid.fees + paid.tax + (paid.commissions?.amount ?? 0);
    const ofAccount = (amount: number) => (p.total > 0 && amount !== 0 ? t("{share} of the account", { share: tenths(amount / p.total) }) : undefined);
    incomeStats.push({ label: t("Dividends and interest"), value: signed(income), tone: toned(income), note: ofAccount(income) });
    incomeStats.push({ label: t("Fees, tax and commissions"), value: signed(costs), tone: costs < 0 ? "negative" : undefined, note: ofAccount(costs) });
  }
  // The last twelve months the account was valued in, each with its gain.
  const byMonth = p.monthly;
  const lacking = p.missing.map(lackOf).filter((text): text is string => !!text);
  // Each kind's colour, largest first, shared by the legend, the marks and the share bars.
  const kindColour = (group: string) => seriesColour(sums.byKind.findIndex((part) => part.key === group));
  const largest = Math.max(1, p.largest);
  const shareBar = (part: number | null, value: number, colour: string) =>
    part !== null ? (
      <span className="holdings-share">
        <span className="holdings-track" aria-hidden>
          <i style={{ width: `${Math.max(2, (value / largest) * 100)}%`, background: colour }} />
        </span>
        <span dir="ltr">{percent(part)}</span>
      </span>
    ) : null;
  const positionKey = (h: HoldingsPage["holdings"][number]) => `${h.symbol}:${h.currency}:${h.one}`;
  const charted = dates.length > 2;
  const pick = (next: Chart) => {
    store(CHART, next);
    setChart(next);
  };
  // The two figures at the top. With a chart under them they choose what it draws.
  const worth = (
    <>
      <span className="holdings-label">{t("Value")}</span>
      <strong>{money(p.total)}</strong>
    </>
  );
  const gained = did && (
    <>
      <span className="holdings-label">
        {period === "all"
          ? t("Gain since {date}", { date: formatDate(did.from) })
          : { "1m": t("Gain over 1 month"), "3m": t("Gain over 3 months"), ytd: t("Gain this year"), "1y": t("Gain over 1 year") }[period]}
      </span>
      <strong className={tone(did.gain)}>
        {signed(did.gain)}
        {did.rate !== null && (
          <span className={`holdings-pill ${tone(did.gain)}`} dir="ltr">
            {percent(did.rate, true)}
          </span>
        )}
      </strong>
    </>
  );
  return (
    <section className={`holdings-view${paid || byMonth.length > 1 ? " with-side" : ""}`} aria-label={t("Holdings")}>
      {bar}
      {/* How it's doing: its value and its gain over a period, a chart of either, and the figures. */}
      <div className="holdings-card holdings-perf">
      <div className="holdings-tabs">
        {charted ? (
          <div
            className="holdings-metrics"
            role="tablist"
            aria-label={t("Chart")}
            onKeyDown={(e) => {
              if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
              e.preventDefault();
              pick(chart === "value" ? "gain" : "value");
              (e.currentTarget.querySelector<HTMLElement>('[aria-selected="false"]') ?? e.currentTarget).focus();
            }}
          >
            <button type="button" role="tab" className="holdings-metric" aria-selected={chart === "value"} tabIndex={chart === "value" ? 0 : -1} onClick={() => pick("value")}>
              {worth}
            </button>
            <button type="button" role="tab" className="holdings-metric" aria-selected={chart === "gain"} tabIndex={chart === "gain" ? 0 : -1} onClick={() => pick("gain")}>
              {gained}
            </button>
          </div>
        ) : (
          <div className="holdings-metrics">
            <div className="holdings-metric">{worth}</div>
            {gained && <div className="holdings-metric">{gained}</div>}
          </div>
        )}
        <Tabs<Period>
          label={t("Period")}
          value={period}
          onChange={setPeriod}
          tabs={PERIODS.map((p) => ({ id: p, label: periodName[p] }))}
          className="holdings-period"
        />
      </div>
      <div className="holdings-body">
      {lacking.length > 0 && (
        <div className="notice holdings-notice">
          <p>{t("Your Flex Query leaves some sections out, so parts of this page are missing:")}</p>
          <ul>
            {lacking.map((text) => (
              <li key={text}>{text}</li>
            ))}
          </ul>
        </div>
      )}
      {fresh(review) > 0 && (
        <p className="notice holdings-notice">
          {t("Interactive Brokers lists money you moved in or out that isn't in your budget yet. It stands in the account as Money moved in or out until you review it; the gain already leaves it out.")}
        </p>
      )}
      {p.estimated.length > 0 && (
        <p className="notice holdings-notice">
          {tn(
            p.estimated.length,
            "On {count} day the value jumped with no deposit or transfer listed, so that change is counted as money moved, not as a gain. Add the Transfers section to your Flex Query for exact figures.",
            "On {count} days the value jumped with no deposit or transfer listed, so those changes are counted as money moved, not as gains. Add the Transfers section to your Flex Query for exact figures.",
          )}
        </p>
      )}
      {p.estimated.length > 0 && (
        <details className="holdings-earned">
          <summary>{t("The days counted as money moved")}</summary>
          <table className="holdings-table">
            <tbody>
              {p.estimated.map(({ date, amount }) => (
                <tr key={date}>
                  <td>{formatDate(date)}</td>
                  <td className={`num ${tone(amount)}`}>{signed(amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      )}
      {did && charted && (
        <div className="holdings-chart">
          {chart === "value" && (
            <Legend
              items={[
                { label: t("Value"), color: "var(--accent)", line: true },
                { label: t("Value before gains"), color: "var(--muted)", line: true, dashed: true },
              ]}
            />
          )}
          {chart === "value" ? (
            // What it's worth, over what it would be worth with no gain or loss since the period
            // began: the gap between the two lines is the gain. The axis fits the values, since
            // one from zero would flatten an account that moves by a hundredth of its size.
            <LinesChart
              keys={dates}
              series={[
                { key: "value", label: t("Value"), color: "var(--accent)", values },
                // No line where more has been taken out than was put in (an emptied account
                // that had gained): under zero it means nothing, and would stretch the axis.
                {
                  key: "before",
                  label: t("Value before gains"),
                  color: "var(--muted)",
                  values: values.map((v, i) => (v >= 0 && v < gains[i] ? null : v - gains[i])),
                  dashed: true,
                },
              ]}
              label={t("The account's value from {from} to {to}", { from: formatDate(did.from), to: formatDate(did.to) })}
              format={(v) => money(v)}
              height={210}
              tick={dayTick}
              title={formatDate}
              wash="fade"
              zero={false}
              ends={false}
              more={(i) => [{ label: t("Gain"), value: gainText(gains[i]) }]}
            />
          ) : (
            <LinesChart
              keys={dates}
              series={[{ key: "gain", label: t("Gain"), color: "var(--positive)", below: "var(--negative)", values: gains }]}
              label={t("The account's gain from {from} to {to}", { from: formatDate(did.from), to: formatDate(did.to) })}
              format={gainText}
              height={210}
              tick={dayTick}
              title={formatDate}
              wash="fade"
              ends={false}
              more={(i) => [{ label: t("Value"), value: money(values[i]) }]}
            />
          )}
        </div>
      )}
      <Stats items={periodStats} />
      </div>
      </div>
      {/* What it holds: what it is made of and a few facts about it, then each holding. Nothing
          for an account whose value alone is known. */}
      {p.holdings.length + p.cash.length > 0 && (
      <div className="holdings-card holdings-holds">
        <div className="holdings-holds-head">
          <h3>{t("What it holds")}</h3>
          {p.holdings.length > 0 && <span className="holdings-label">{tn(p.holdings.length, "{count} holding", "{count} holdings")}</span>}
        </div>
        <div className="holdings-alloc">
          {kinds.length > 1 && (
            <div className="holdings-ring">
              <Donut
                slices={kinds.map((part) => ({ key: part.key, label: kindName(part.key), value: part.value, color: kindColour(part.key) }))}
                label={t("What the account is made of, by kind")}
                format={(v) => money(v)}
                center={{ value: percent(kinds[0].share), label: kindName(kinds[0].key) }}
                size={124}
              />
              <ul className="holdings-kinds" aria-label={t("By kind")}>
                {kinds.map((part) => (
                  <li key={part.key}>
                    <i aria-hidden style={{ background: kindColour(part.key) }} />
                    {kindName(part.key)} <span className="amount">{money(part.value)}</span> <span dir="ltr">{percent(Math.max(part.share, 0))}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
          <Stats items={holdStats} className="facts" />
        </div>
        {currencies.length > 1 && (
          <ul className="holdings-kinds" aria-label={t("By currency")}>
            {currencies.map((part) => (
              <li key={part.key} dir="ltr">
                {part.key} <span>{percent(Math.max(part.share, 0))}</span>
              </li>
            ))}
          </ul>
        )}
        {p.holdings.length + p.cash.length > 0 && (
          <table className="holdings-table">
            <thead>
              <tr>
                {heading("name", t("Holding"))}
                <th className="num share">{t("Share")}</th>
                {heading("value", t("Value"), "num")}
                {heading("gain", t("Gain"), "num")}
                {heading("rate", t("Return"), "num rate")}
              </tr>
            </thead>
            <tbody>
              {shown.map((h) => {
                const group = h.group;
                const r = h.rate;
                return (
                  <tr key={positionKey(h)}>
                    <td>
                      <div className="holdings-holding">
                        <Badge group={group} symbol={h.symbol} colour={kindColour(group)} />
                        <div>
                          <span className="holdings-name">{h.name}</span>
                          <small>
                            {whatItIs(h) && `${whatItIs(h)} · `}
                            <span dir="ltr">
                              {/* A bond's name is its symbol, and an option's code only spells its
                                  name out again at length: the symbol is shown when it adds something. */}
                              {h.name !== h.symbol && h.symbol.length <= 12 ? `${h.symbol} · ` : ""}
                              {plain(h.quantity)} × {plain(h.price)} {h.currency}
                            </span>
                          </small>
                        </div>
                      </div>
                    </td>
                    <td className="num share">{shareBar(h.share, h.value, kindColour(group))}</td>
                    <td className="num">{money(h.value)}</td>
                    <td className={`num ${h.gain === null ? "" : tone(h.gain)}`}>{h.gain === null ? "" : signed(h.gain)}</td>
                    <td className="num rate">
                      {r !== null && h.gain !== null && (
                        <span className={`holdings-pill ${tone(h.gain)}`} dir="ltr">
                          {percent(r, true)}
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
              {more > 0 && (
                <tr className="holdings-more">
                  <td colSpan={5}>
                    <button className="link-button" onClick={() => setAll(!all)}>
                      {all ? t("Show fewer") : tn(more, "Show {count} more holding", "Show {count} more holdings")}
                    </button>
                  </td>
                </tr>
              )}
              {p.cash.map((c) => (
                <tr key={`cash:${c.currency}`}>
                  <td>
                    <div className="holdings-holding">
                      <Badge group="Cash" colour={kindColour("Cash")} />
                      <div>
                        <span className="holdings-name">{t("Cash")}</span>
                        <small>
                          <span dir="ltr">
                            {plain(c.amount, 2)} {c.currency}
                          </span>
                        </small>
                      </div>
                    </div>
                  </td>
                  <td className="num share">{shareBar(c.share, c.value, kindColour("Cash"))}</td>
                  <td className="num">{money(c.value)}</td>
                  <td />
                  <td className="rate" />
                </tr>
              ))}
              {other !== 0 && (
                <tr>
                  <td>
                    <div className="holdings-holding">
                      <Badge group="Accrued" colour={kindColour("Accrued")} />
                      <div>
                        <span className="holdings-name">{t("Accrued")}</span>
                        <small>{t("Dividends and interest earned but not yet paid")}</small>
                      </div>
                    </div>
                  </td>
                  <td className="num share">{shareBar(ofTotal(other), other, kindColour("Accrued"))}</td>
                  <td className="num">{money(other)}</td>
                  <td />
                  <td className="rate" />
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
      )}
      {(paid || byMonth.length > 1) && (
      <div className="holdings-side">
      {byMonth.length > 1 && (
        <div className="holdings-card">
          <h3>{t("Month by month")}</h3>
          <ColumnChart
            months={byMonth.map((m) => m.month)}
            series={[
              { key: "gain", label: t("Gain"), color: "var(--positive)", values: byMonth.map((m) => Math.max(0, m.gain)) },
              { key: "loss", label: t("Loss"), color: "var(--negative)", values: byMonth.map((m) => Math.min(0, m.gain)) },
            ]}
            label={t("The account's gain in each month")}
            format={gainText}
            height={180}
            total={false}
          />
        </div>
      )}
      {paid && (
        <div className="holdings-card holdings-income">
          <h3>{t("Income and costs")}</h3>
          <Stats items={incomeStats} className="two" />
          <table className="holdings-table">
            <thead>
              <tr>
                <th>
                  {t("{from} to {to}", { from: monthName(paid.from, "short"), to: monthName(paid.to, "short") })}
                </th>
                <th className="num">{t("Amount")}</th>
              </tr>
            </thead>
            <tbody>
              {(
                [
                  [t("Dividends"), paid.dividends, ""],
                  [t("Interest"), paid.interest, ""],
                  [t("Tax withheld"), paid.tax, ""],
                  [t("Fees"), paid.fees, t("Market data, advisers and other charges apart from trades")],
                  [t("Other"), paid.other, ""],
                ] as [string, number, string][]
              )
                .filter(([, amount]) => amount !== 0)
                .map(([label, amount, note]) => (
                  <tr key={label}>
                    <td>
                      {label}
                      {note && <small>{note}</small>}
                    </td>
                    <td className={`num ${tone(amount)}`}>{signed(amount)}</td>
                  </tr>
                ))}
              {paid.commissions && (
                <tr>
                  <td>
                    {t("Commissions on trades")}
                    <small>
                      {t("{from} to {to}", { from: formatDate(paid.commissions.from), to: formatDate(paid.commissions.to) })}
                    </small>
                  </td>
                  <td className={`num ${tone(paid.commissions.amount)}`}>{signed(paid.commissions.amount)}</td>
                </tr>
              )}
            </tbody>
          </table>
          <p className="loan-note">
            {t("All of these are already in the gain above. Dividends, interest, tax and fees arrive as cash; a trade's commission is part of what the trade cost.")}
          </p>
        </div>
      )}
      </div>
      )}
      {after}
    </section>
  );
}
