import { accountType, inNetWorth, isLiability, sectionOf } from "../accounts";
import { msg } from "../i18n";
import type { Account, Portfolio, PortfolioAccount, PortfolioHolding, PortfolioParts } from "../types";
import { firstDay, sum, type ReportData } from "./shared";

// The balance sheet's rows: what is owned by what the money is in, and what is owed by account
// type. An investment account's balance is spread over the kinds it holds, its cash and its
// accrued income; every other account is one row.

// The last section on each side holds accounts on the other side of zero from their type.
// Section names are keys (accounts.ts's sectionOf gives the same English); they're translated when shown.
export const IN_CREDIT = msg("Accounts in credit"),
  OVERDRAWN = msg("Overdrawn accounts");
const CASH = msg("Cash and bank"),
  OTHER_INVESTMENTS = msg("Other investments"),
  RECEIVABLES = msg("Receivables");
/** The section accounts.ts gives an investment or retirement account: here its balance goes by what it holds. */
const BY_TYPE = msg("Investments");
/** The kinds of holding with a section of their own, as the engine names them (investment_stats.rs `kind`). */
const KINDS = ["Stocks", "Funds", "Bonds", "Options", "Futures", "Currencies", "Crypto", "Commodities", "CFDs"];
export const ASSET_SECTIONS = [CASH, ...KINDS, OTHER_INVESTMENTS, msg("Property"), msg("Vehicles"), RECEIVABLES, msg("Other assets"), IN_CREDIT];
export const DEBT_SECTIONS = [msg("Credit cards"), msg("Lines of credit"), msg("Mortgages"), msg("Loans"), msg("Other debts"), OVERDRAWN];
/** The section whose money can be spent straight away: bank accounts, and cash at brokers. */
export const LIQUID = CASH;

/** An account on the statement, as one row: debts read as what's owed, assets as what's held. */
export type AccountLine = { a: Account; section: string; debt: boolean; start: number; end: number };
/**
 * A row of the statement: a whole account, or one part of an investment account (the holdings of
 * a kind, its cash, its accrued income, or what its balance has moved since its prices, dated
 * `asOf`). `start` is null where what the account held then isn't kept.
 */
export type SheetLine = {
  key: string;
  a: Account;
  section: string;
  part: "whole" | "kind" | "cash" | "accrued" | "rest";
  asOf?: string;
  start: number | null;
  end: number;
};
export type Sheet = {
  assets: SheetLine[];
  debts: SheetLine[];
  /** Accounts left out of net worth, listed apart. */
  apart: AccountLine[];
  /** From the accounts' balances, so they are the same however the assets are split. */
  totals: { assets: { start: number; end: number }; debts: { start: number; end: number } };
  /** Investment accounts whose start isn't known by type, and ones that couldn't be split at all. */
  startUnknown: Account[];
  unsplit: Account[];
  /** How the investment accounts among the assets changed: by money put in, and by markets. */
  investments: { change: number; put: number; gain: number } | null;
  /** Whether an account sits on the other side of zero from its type. */
  crossed: boolean;
};

type Piece = { section: string; part: SheetLine["part"]; value: number; asOf: string };
/** A balance's parts by row: the holdings of each kind, cash, accrued income, and whatever is left of `balance`. */
function pieces(parts: PortfolioParts, balance: number) {
  const out = new Map<string, Piece>();
  const add = (key: string, section: string, part: Piece["part"], value: number) => {
    if (!value) return;
    const had = out.get(key);
    if (had) had.value += value;
    else out.set(key, { section, part, value, asOf: parts.asOf });
  };
  for (const kind of parts.byKind) {
    // Holdings a source gave no kind for, and kinds with no section, are other investments.
    const section = KINDS.includes(kind.key) ? kind.key : OTHER_INVESTMENTS;
    add(`kind:${section}`, section, "kind", kind.value);
  }
  add("cash", CASH, "cash", parts.cash);
  add("accrued", RECEIVABLES, "accrued", parts.accrued);
  // Whatever the parts leave of the balance, so the account's rows always add up to it.
  add("rest", OTHER_INVESTMENTS, "rest", balance - sum([...out.values()].map((p) => p.value)));
  return out;
}

/**
 * The statement for a report's range. `portfolio` gives each investment account's parts at the
 * start and the end; without it (no holdings, or it couldn't be read) every account is one row.
 */
export function balanceSheet(
  data: ReportData,
  accounts: Account[],
  chosen: Set<string>,
  portfolio: Portfolio | null,
): Sheet {
  const first = data.months[0],
    last = data.months[data.months.length - 1];
  const opening = data.openingAccounts ?? {};
  const whole: AccountLine[] = accounts
    .filter((a) => (!chosen.size || chosen.has(a.id)) && a.id in last.accounts)
    .map((a) => {
      const end = last.accounts[a.id] ?? 0,
        start = opening[a.id] ?? 0;
      const type = accountType(a, end);
      // The balance decides the side, as it does for net worth: an overdrawn bank account is a
      // debt and a loan in credit is an asset. Debts read as what's owed; assets as what's held.
      const debt = end === 0 ? isLiability(type) : end < 0;
      const section = debt === isLiability(type) ? sectionOf(type) : debt ? OVERDRAWN : IN_CREDIT;
      const sign = debt ? -1 : 1;
      return { a, section, debt, start: start * sign, end: end * sign };
    })
    .filter((l) => l.start || l.end || !l.a.archived);
  const counted = whole.filter((l) => inNetWorth(l.a));
  const owned = counted.filter((l) => !l.debt),
    owed = counted.filter((l) => l.debt);
  const held = new Map<string, PortfolioAccount>((portfolio?.accounts ?? []).map((p) => [p.accountId, p]));
  const startUnknown: Account[] = [],
    unsplit: Account[] = [];
  const one = (l: AccountLine, section = l.section): SheetLine => ({ key: l.a.id, a: l.a, section, part: "whole", start: l.start, end: l.end });
  const assets = owned.flatMap((l): SheetLine[] => {
    const p = held.get(l.a.id);
    // An investment account nothing breaks down is one row among the other investments.
    if (!p) return [one(l, l.section === BY_TYPE ? OTHER_INVESTMENTS : l.section)];
    if (!p.endParts) {
      unsplit.push(l.a);
      return [one(l, OTHER_INVESTMENTS)];
    }
    // One whose value alone is known has nothing to be spread over.
    if (!p.endParts.byKind.some((kind) => kind.value) && !p.endParts.cash && !p.endParts.accrued) return [one(l, OTHER_INVESTMENTS)];
    const end = pieces(p.endParts, l.end);
    // With no record of the start, it is known only for an account that held nothing before the range.
    const start = p.startParts
      ? pieces(p.startParts, l.start)
      : l.start === 0 && p.joined >= firstDay(first.month)
        ? new Map<string, Piece>()
        : null;
    if (!start) startUnknown.push(l.a);
    return [...new Set([...end.keys(), ...(start?.keys() ?? [])])].map((key) => {
      const at = end.get(key) ?? start!.get(key)!;
      return {
        key: `${l.a.id}:${key}`,
        a: l.a,
        section: at.section,
        part: at.part,
        asOf: at.asOf,
        start: start ? (start.get(key)?.value ?? 0) : null,
        end: end.get(key)?.value ?? 0,
      };
    });
  });
  const invested = owned.filter((l) => held.has(l.a.id));
  const change = sum(invested.map((l) => l.end - l.start));
  const gain = sum(invested.map((l) => held.get(l.a.id)!.gain));
  const total = (ls: AccountLine[], key: "start" | "end") => sum(ls.map((l) => l[key]));
  return {
    assets,
    debts: owed.map((l) => one(l)),
    apart: whole.filter((l) => !inNetWorth(l.a)),
    totals: {
      assets: { start: total(owned, "start"), end: total(owned, "end") },
      debts: { start: total(owed, "start"), end: total(owed, "end") },
    },
    startUnknown,
    unsplit,
    investments: invested.length ? { change, put: change - gain, gain } : null,
    crossed: whole.some((l) => l.section === OVERDRAWN || l.section === IN_CREDIT),
  };
}

/** A holding as a row of its section: what the accounts hold of it together, today. */
export type HeldLine = { key: string; section: string; holding: PortfolioHolding };
/**
 * The holdings each investment section lists in place of its accounts' lines, for a range that
 * ends this month (`portfolio.holdings` are today's). A section keeps its accounts' lines unless
 * every holding in it is in accounts spread over the sections here and the holdings add up to
 * those lines exactly, so a section's rows always add up to it.
 */
export function heldLines(sheet: Sheet, portfolio: Portfolio): Map<string, HeldLine[]> {
  const spread = new Set(sheet.assets.filter((l) => l.part === "kind").map((l) => l.a.id));
  const out = new Map<string, HeldLine[]>();
  const unsure = new Set<string>();
  for (const holding of portfolio.holdings) {
    // Holdings a source gave no kind for, and kinds with no section, are other investments.
    const section = KINDS.includes(holding.group) ? holding.group : OTHER_INVESTMENTS;
    if (!holding.accounts.every((id) => spread.has(id))) unsure.add(section);
    out.set(section, [...(out.get(section) ?? []), { key: `holding:${holding.key}`, section, holding }]);
  }
  for (const [section, rows] of [...out]) {
    const lines = sheet.assets.filter((l) => l.section === section && l.part === "kind");
    if (unsure.has(section) || sum(lines.map((l) => l.end)) !== sum(rows.map((r) => r.holding.value))) out.delete(section);
  }
  return out;
}

/** A section's rows added up; its start is unknown when any row's is. */
export function sectionTotal(lines: SheetLine[]) {
  return {
    start: lines.some((l) => l.start === null) ? null : sum(lines.map((l) => l.start ?? 0)),
    end: sum(lines.map((l) => l.end)),
  };
}
