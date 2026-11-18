import { invoke } from "@tauri-apps/api/core";
import { change, read, type BankRow } from "./bridge";
import { apply, type Patch } from "./patch";
import type { ImportChoice } from "./BankImport";
import type { Budget } from "./types";

/** An account in the last statement from Interactive Brokers. */
export type IbkrAccount = {
  /** Interactive Brokers' account id (U1234567). */
  id: string;
  /** The alias given to it there, or empty. */
  alias: string;
  /** Its own currency, or empty when the statement doesn't say. */
  currency: string;
  /** What it's worth in that currency, in cents. */
  value: number;
  asOf: string;
  positions: number;
  /** Sections the Flex Query leaves out, by the name Client Portal shows. */
  missing: string[];
  /** The statement carries the days' exchange rates (Include Currency Rates is on). */
  rates: boolean;
  /** When Interactive Brokers made the statement, on its own clock ("2026-11-16 13:45"), or empty. */
  generated: string;
};
/** A login at Interactive Brokers kept on this computer: its Flex Query, never its token. */
export type IbkrLogin = {
  /** The Flex Query's id, which names the login here. */
  query: string;
  /** The Interactive Brokers accounts its last statement named. */
  accounts: string[];
  /** Why it couldn't be fetched the last time it was asked, or null. */
  error: string | null;
};
export type IbkrStatus = {
  /** What protects the token (Keychain, Windows encrypted storage), or null where there's no such store. */
  store: string | null;
  connected: boolean;
  /** The logins kept here, in the order they were connected. */
  logins: IbkrLogin[];
  /** The accounts in the last statement this session; empty until there is one. */
  accounts: IbkrAccount[];
  fetchedAt: string | null;
  /** The statement came from a file rather than the web service. */
  fromFile: boolean;
};
/** An account in the last statement, against the budget. */
export type IbkrReviewAccount = IbkrAccount & {
  /** The budget account it fills, if any. */
  accountId: string | null;
  /** The currencies an exchange rate is needed between before it can be brought in. */
  needsRate: [from: string, to: string] | null;
  /** Its deposits, withdrawals and transfers, to review like a bank's rows. */
  rows: BankRow[];
  error: string | null;
  /** Read from what the budget keeps, not a statement this session: its login is on another device, or nothing was fetched here yet. */
  kept?: boolean;
};
export type IbkrReview = { accounts: IbkrReviewAccount[]; fetchedAt: string };
/** Which budget account an Interactive Brokers account fills. */
export type IbkrLink = {
  source: string;
  /** An existing tracking account's id, or the id for a new one. */
  account: string;
  create?: boolean;
  name?: string;
  /** One unit of the account's currency in the budget's, when the statement has no rate. */
  rate?: string;
};

/** Interactive Brokers through its Flex Web Service (see ../crates/urbudget-services/src/ibkr.rs). */
export const ibkr = {
  status: () => invoke<IbkrStatus>("ibkr_status"),
  /**
   * Checks the token and query by fetching the statement, and keeps them if it comes: beside
   * the logins already here, or in place of the one they renew.
   */
  connect: (token: string, query: string) => invoke<IbkrStatus>("ibkr_connect", { token, query }),
  /** Fetches every login's statement again with the tokens that are kept. It can take a minute. */
  fetch: () => invoke<IbkrStatus>("ibkr_fetch"),
  /** Reads a statement saved from Client Portal, in place of a fetch. */
  file: (content: string) => invoke<IbkrStatus>("ibkr_file", { content }),
  /** Forgets one login, by its query id, or all of them. */
  disconnect: (query?: string) => invoke<IbkrStatus>("ibkr_disconnect", { query: query ?? null }),
  /** Opens Interactive Brokers' page on making a Flex Web Service token. */
  openGuide: () => invoke<void>("ibkr_open"),
  /** The last statement's accounts against `budget`: what's linked, what needs a rate, deposits to review. */
  review: (budget: Budget) => read<IbkrReview>(budget, "ibkr_review"),
  /**
   * Brings the last statement into every linked account, and into the ones in `links`. A
   * statement that changes nothing gives `budget` itself back, so the caller can tell.
   */
  apply: async (budget: Budget, links: IbkrLink[] = []) => {
    const patch = await read<Patch>(budget, "ibkr_apply", { links });
    return Object.values(patch).every((part) => part == null) ? budget : apply(budget, patch);
  },
  /** Imports reviewed deposits and withdrawals into `account`. */
  import: (budget: Budget, account: string, choices: ImportChoice[]) => change(budget, "ibkr_import", { account, choices }),
};
