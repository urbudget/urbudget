import { useEffect, useState } from "react";
import { read } from "../bridge";
import { errorText } from "../format";
import type { Account, Budget, Portfolio } from "../types";

/** The accounts something reports holdings for, in the budget's order. */
export const holdingAccounts = (budget: Pick<Budget, "accounts" | "investments">): Account[] =>
  budget.accounts.filter((a) => budget.investments?.some((held) => held.accountId === a.id));

/**
 * The investments report for these months and accounts (empty for all), read from the engine as
 * the forecast reads its own figures. It belongs to its exact budget and filters: while another
 * is on its way there is no data, only `error` once a read has failed. `on` false asks for nothing.
 */
export function usePortfolio(budget: Budget, filter: { from: string; to: string; accountIds: string[] }, on = true) {
  const [result, setResult] = useState<{ budget: Budget; key: string; attempt: number; data?: Portfolio; error?: string } | null>(null);
  const [attempt, setAttempt] = useState(0);
  const key = JSON.stringify({ from: filter.from, to: filter.to, accountIds: filter.accountIds, categoryIds: [] });
  const matches = on && result?.budget === budget && result.key === key && result.attempt === attempt;
  useEffect(() => {
    if (!on) return;
    let cancelled = false;
    const controller = new AbortController();
    read<Portfolio>(budget, "investments_report", { filter: JSON.parse(key) }, controller.signal)
      .then((data) => !cancelled && setResult({ budget, key, attempt, data }))
      .catch((e) => !cancelled && setResult({ budget, key, attempt, error: errorText(e) }));
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [budget, key, attempt, on]);
  return {
    data: matches ? result.data : undefined,
    error: matches ? result.error : undefined,
    retry: () => setAttempt((n) => n + 1),
  };
}
