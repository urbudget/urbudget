import type { Account } from "./types";
import { t } from "./i18n";

/** How a transfer reads from one of its two accounts. */
export type TransferView = {
  /** The other account's id and name. */
  account: string;
  peer: string;
  direction: "to" | "from";
  /** Money going from a budget account to a credit card: paying the card. */
  card: boolean;
  /** "To: Savings", "From: Checking". */
  label: string;
};

/**
 * A transfer seen from `self` (the account whose register shows it), with `amount` as that
 * account sees it: negative when money leaves it. Null when it isn't a transfer.
 */
export function transferView(
  accounts: Map<string, Pick<Account, "name" | "kind">>,
  self: string | null | undefined,
  peer: string | null | undefined,
  amount: number,
): TransferView | null {
  if (!peer) return null;
  const other = accounts.get(peer);
  const mine = self ? accounts.get(self) : undefined;
  const direction = amount < 0 ? "to" : "from";
  const card =
    direction === "to" ? other?.kind === "credit" && mine?.kind === "cash" : mine?.kind === "credit" && other?.kind === "cash";
  const name = other?.name ?? t("another account");
  const label = direction === "to" ? t("To: {account}", { account: name }) : t("From: {account}", { account: name });
  return { account: peer, peer: name, direction, card, label };
}
