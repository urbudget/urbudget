import { invoke } from "@tauri-apps/api/core";
import { change, read, type BankRow } from "./bridge";
import type { ImportChoice } from "./BankImport";
import { apply, type Patch } from "./patch";
import type { Budget } from "./types";

/** An account at SimpleFIN Bridge, for choosing which budget account it fills. */
export type SimpleFinAccount = {
  id: string;
  name: string;
  /** The bank. */
  org: string;
  /** Its ISO code, or null for a currency of its own. */
  currency: string | null;
  balance: number | null;
  balanceDate: string | null;
};
export type SimpleFinStatus = {
  /** What protects the connection (Keychain, Windows encrypted storage), or null where bank sync isn't offered. */
  store: string | null;
  connected: boolean;
  /** As SimpleFIN last listed them this session; empty until it has. */
  accounts: SimpleFinAccount[];
  messages: string[];
  requestsToday: number;
  dailyLimit: number;
};
/** One linked account's part of a sync. */
export type AccountReview = {
  accountId: string;
  /** The SimpleFIN account's name. */
  name: string;
  rows: BankRow[];
  balance: number | null;
  balanceDate: string | null;
  /** Why this account can't be reviewed. */
  error: string | null;
  /** How many holdings the bank sent for an investment account: with any, `holdings` brings them in. */
  holdings?: number;
};
export type Synced = {
  reviews: AccountReview[];
  messages: string[];
  /** The day of the download; an imported account is synced through it. */
  through: string;
  requestsToday: number;
  dailyLimit: number;
};

/** Bank sync through SimpleFIN Bridge (see ../crates/urbudget-services/src/simplefin.rs). */
export const simplefin = {
  status: () => invoke<SimpleFinStatus>("simplefin_status"),
  /** Claims a setup token (it works once) and lists the accounts. */
  connect: (token: string) => invoke<SimpleFinStatus>("simplefin_connect", { token }),
  /** Lists the accounts again: one request. */
  refresh: () => invoke<SimpleFinStatus>("simplefin_accounts"),
  disconnect: () => invoke<SimpleFinStatus>("simplefin_disconnect"),
  /** Opens SimpleFIN Bridge's page for making a setup token. */
  openBridge: () => invoke<void>("simplefin_open"),
  /**
   * Every linked account's review rows. `refresh` downloads (one request, more for a long first
   * sync); without it the last download is checked against `budget` again.
   */
  sync: (budget: Budget, refresh: boolean) => read<Synced>(budget, "simplefin_sync", { refresh }),
  /** Imports the reviewed rows into `account` and records it as synced through `through`. */
  import: (budget: Budget, account: string, choices: ImportChoice[], through: string) =>
    change(budget, "simplefin_import", { account, choices, through }),
  /**
   * Brings what the linked investment accounts hold into the budget, from the last download. A
   * download that says what the last one did gives `budget` itself back, so nothing is saved.
   */
  holdings: async (budget: Budget) => {
    const patch = await read<Patch>(budget, "simplefin_holdings");
    return Object.values(patch).every((part) => part == null) ? budget : apply(budget, patch);
  },
};
