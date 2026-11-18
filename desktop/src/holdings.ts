import { invoke } from "@tauri-apps/api/core";
import { change } from "./bridge";
import { activeFormat } from "./format";
import { ltr, msg, t } from "./i18n";
import type { Budget, Holdings } from "./types";

/** One thing a file says an account holds: its figures as plain decimals, in the file's currency. */
export type Position = {
  symbol: string;
  name: string;
  /** STK, FUND, BOND, OPT or CRYPTO; empty when the file doesn't say. */
  kind: string;
  currency: string;
  quantity: string;
  price: string;
  value: string;
  /** What it cost in all; empty when the file doesn't say. */
  cost: string;
};
/** What a broker's file says an account holds (positions_file.rs). */
export type PositionsFile = {
  format: "csv" | "ofx";
  /** The day the figures are from, when the file says; else empty. */
  asOf: string;
  /** The currency the values are in, when the file says; else empty. */
  currency: string;
  positions: Position[];
  /** Cash not invested; empty when the file lists none. */
  cash: string;
  /** Everything it lists added up. */
  value: string;
  /** Rows that looked like holdings and had no value that could be read. */
  skipped: number;
};
/** What an account holds on a day, to record (investments.rs `Snapshot`). */
export type Snapshot = {
  source: "file" | "manual";
  sourceAccount?: string;
  asOf: string;
  /** The currency the figures are in, when it isn't the budget's, and what one unit of it is worth in the budget's. */
  currency?: string;
  rate?: string;
  positions?: Position[];
  cash?: string;
  /** The account's whole value: all there is for a value typed in. */
  total?: string;
};

/** Holdings from a positions file or a value typed in (see ../crates/urbudget-core/src/investments.rs). */
export const holdings = {
  /** Reads a positions file's text: nothing changes. */
  file: (content: string) => invoke<PositionsFile>("holdings_file", { content }),
  /** Records what `account` holds on a day; its balance follows that value from then on. */
  record: (budget: Budget, account: string, snapshot: Snapshot) => change(budget, "holdings_record", { account, snapshot }),
};

/** A quantity or price as the source wrote it, to at most `places` decimals, in the budget's number format. */
export const plain = (text: string, places = 4) => {
  const n = Number(text);
  if (!Number.isFinite(n)) return text;
  const [whole, frac = ""] = n.toFixed(places).split(".");
  const { decimal, group } = activeFormat().number;
  const digits = whole.replace("-", "").replace(/\B(?=(\d{3})+(?!\d))/g, group);
  const rest = frac.replace(/0+$/, "");
  return `${n < 0 ? "-" : ""}${digits}${rest ? decimal + rest : ""}`;
};

/**
 * What the engine calls kinds of holding, a group of them and one (investment_stats.rs `kind`),
 * with "Cash" and "Accrued" beside them: English keys, translated where they're shown.
 */
const KIND_NAMES = new Set<string>([
  msg("Stocks"),
  msg("Stock"),
  msg("Funds"),
  msg("Fund"),
  msg("Bonds"),
  msg("Bond"),
  msg("Options"),
  msg("Option"),
  msg("Futures"),
  msg("Future"),
  msg("Currencies"),
  msg("Currency"),
  msg("Crypto"),
  msg("Commodities"),
  msg("Commodity"),
  msg("CFDs"),
  msg("CFD"),
  msg("Holdings"),
  msg("Other holdings"),
  msg("Other"),
  msg("Cash"),
  msg("Accrued"),
  // What a holding is, more exactly (investment_stats.rs `instrument`).
  msg("ETF"),
  msg("Exchange-traded note"),
  msg("REIT"),
  msg("Depositary receipt"),
  msg("Preferred share"),
  msg("Right"),
  msg("Closed-end fund"),
  msg("Treasury bill"),
  msg("Government bond"),
  msg("Corporate bond"),
  msg("Call option"),
  msg("Put option"),
]);
/** A kind of holding as the engine names it ("Stocks", "Stock", "ETF"), in the language in use; a name it doesn't know, as it is. */
export const kindName = (key: string) => (KIND_NAMES.has(key) ? t(key) : key);
/** What a holding is: its instrument where the engine gives one, else one of its kind ("Stock"). */
export const whatItIs = (holding: { instrument?: string; one: string }) => kindName(holding.instrument || holding.one);

/** A decimal as the source wrote it, in the budget's number format: grouped, its decimals kept. */
export const decimal = (text: string) => {
  const parts = /^(-?)(\d+)(?:\.(\d+))?$/.exec(text.trim());
  if (!parts) return text;
  const { decimal: mark, group } = activeFormat().number;
  return `${parts[1]}${parts[2].replace(/\B(?=(\d{3})+(?!\d))/g, group)}${parts[3] ? mark + parts[3] : ""}`;
};
/** An amount in its own currency ("61,476.00 USD"), kept left to right in Arabic too. */
export const inCurrency = (amount: string, currency: string) => ltr(`${decimal(amount)} ${currency}`);
/**
 * What a holding is worth in its own currency, where that says more than its value in the
 * budget's: nothing when it isn't known or is already in the budget's currency.
 */
export const ownAmount = (amount: string | null | undefined, currency: string, budgetCurrency: string) =>
  amount && currency && currency !== budgetCurrency ? inCurrency(amount, currency) : null;
/**
 * A holding's symbol where it adds to its name: not when it is the name (a bond's is), nor when it
 * is a long code that only spells the name out again (an option's "AAPL 261218C00250000").
 */
export const extraSymbol = (h: { symbol: string; name: string }) =>
  h.symbol && h.name && h.name !== h.symbol && h.symbol.length <= 12 ? h.symbol : "";
/**
 * The line under a holding's name: what it is, its symbol where that adds something, and what it
 * is worth in its own currency where that isn't the budget's ("ETF · VWRA · 61,476.00 USD").
 */
export const holdingLine = (
  h: { instrument?: string; one: string; symbol: string; name: string; amount?: string | null; currency: string },
  budgetCurrency: string,
) => [whatItIs(h), extraSymbol(h), ownAmount(h.amount, h.currency, budgetCurrency)].filter((part): part is string => !!part);
/** An account's cash by currency ("796.50 USD · 1,500.00 SAR"), when some of it isn't in the budget's currency. */
export const cashByCurrency = (cash: { currency: string; amount: string }[] | undefined, budgetCurrency: string) =>
  cash?.some((c) => c.currency !== budgetCurrency) ? cash.map((c) => inCurrency(c.amount, c.currency)).join(" · ") : null;

/** How an account is kept up to date, by where its holdings come from. */
export const sourceName = (held: Pick<Holdings, "source">) =>
  held.source === "ibkr"
    ? "Interactive Brokers"
    : held.source === "simplefin"
      ? t("Bank connection")
      : held.source === "file"
        ? t("Positions file")
        : t("Value you type in");

/** Asks for a positions file: the broker's CSV or spreadsheet of positions, or an OFX/QFX investment statement. */
export const pickPositionsFile = () =>
  new Promise<File | null>((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".csv,.tsv,.txt,.ofx,.qfx,.xlsx,.xlsm,.xls,.ods";
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
