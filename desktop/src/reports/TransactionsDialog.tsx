import { useMemo } from "react";
import { ArrowRight } from "lucide-react";
import { Modal } from "../forms";
import { formatDate, money } from "../format";
import { onBudget } from "../ActivityPopover";
import { TransactionLines, type ActivityLine } from "../TransactionLines";
import { newestFirst } from "../order";
import { effectivePayee, splitRole, UNCATEGORIZED_CATEGORY } from "../accounting";
import { NEEDS_CATEGORY } from "../uncategorized";
import type { Budget, Transaction } from "../types";
import { t, useLanguage } from "../i18n";

/** Which transactions a report figure is made of. Dates are ISO days, inclusive. */
export type LineQuery = {
  from: string;
  to: string;
  payee?: string;
  /** Only these categories (spending). */
  categoryIds?: Set<string>;
  /** Income: money to Ready to Assign instead of spending. */
  income?: boolean;
  /** Only these budget accounts; empty means all. */
  accountIds?: Set<string>;
};

/**
 * The lines behind a report figure: categorized and missing-category spending, or income to
 * Unassigned, including transfers across the budget boundary signed from the budget side.
 * Card payment categories and starting balances never count as period spending or income.
 */
function reportEntries(b: Budget, q: LineQuery) {
  const accounts = new Map(b.accounts.map((a) => [a.id, a]));
  const categories = new Map(b.categories.map((c) => [c.id, c]));
  const name = (id: string | null | undefined) => accounts.get(id ?? "")?.name ?? "";
  const lines: { line: ActivityLine; t: Transaction }[] = [];
  for (const tx of b.transactions) {
    if (tx.date < q.from || tx.date > q.to) continue;
    const source = accounts.get(tx.accountId);
    tx.splits.forEach((s, i) => {
      const other = s.transferAccountId ?? tx.transferAccountId;
      let amount = s.amount,
        account = tx.accountId,
        payee = effectivePayee(tx, s) || t("No payee");
      if (other) {
        const fromBudget = onBudget(source);
        if (fromBudget === onBudget(accounts.get(other))) return;
        amount = fromBudget ? s.amount : -s.amount;
        account = fromBudget ? tx.accountId : other;
        payee = t("Transfer: {account}", { account: name(fromBudget ? other : tx.accountId) });
      } else if (!onBudget(source)) return;
      if (q.accountIds?.size && !q.accountIds.has(account)) return;
      if (q.payee !== undefined && effectivePayee(tx, s) !== q.payee) return;
      const category = s.categoryId ? categories.get(s.categoryId) : undefined;
      const role = splitRole(tx, s, amount);
      if (role === "opening") return;
      if (q.income) {
        if (role !== "income") return;
      } else {
        if (amount === 0) return;
        if (role === "income" || (role === "expense" && (!category || category.paymentAccountId))) return;
        const id = role === "uncategorized" ? UNCATEGORIZED_CATEGORY : category!.id;
        if (q.categoryIds && !q.categoryIds.has(id)) return;
      }
      lines.push({
        t: tx,
        line: {
          key: `${tx.id}:${i}`,
          date: tx.date,
          payee,
          account: name(account),
          memo: s.memo || tx.memo,
          amount,
          category: role === "uncategorized" ? t(NEEDS_CATEGORY) : category?.name ?? t("Unassigned"),
        },
      });
    });
  }
  // Newest first, like the register.
  return lines;
}

export const reportLines = (b: Budget, q: LineQuery): ActivityLine[] =>
  newestFirst(reportEntries(b, q), (l) => l.t).map((l) => l.line);

/** Register navigation uses the same split, income and budget-side transfer rules as the figure. */
export const reportTransactionIds = (b: Budget, q: LineQuery) =>
  new Set(reportEntries(b, q).map((entry) => entry.t.id));

/** The transactions behind a report figure, with their total, and a way into the register. */
export function TransactionsDialog({
  budget,
  title,
  query,
  onClose,
  onOpenRegister,
}: {
  budget: Budget;
  title: string;
  query: LineQuery;
  onClose: () => void;
  onOpenRegister: () => void;
}) {
  const language = useLanguage();
  const { lines, count } = useMemo(() => {
    const entries = reportEntries(budget, query);
    return {
      lines: newestFirst(entries, (entry) => entry.t).map((entry) => entry.line),
      count: new Set(entries.map((entry) => entry.t.id)).size,
    };
  }, [budget.transactions, budget.accounts, budget.categories, query, language]);
  const total = lines.reduce((n, l) => n + l.amount, 0);
  // Spending reads as a positive amount spent; income as received.
  const shown = query.income ? total : -total;
  return (
    <Modal title={title} close={onClose} wide>
      <div className="txn-dialog">
        <p className="muted txn-dialog-range">
          {query.from === query.to ? formatDate(query.from) : `${formatDate(query.from)} – ${formatDate(query.to)}`}
          {query.accountIds?.size ? ` · ${t("chosen accounts")}` : ""}
        </p>
        <dl className="txn-dialog-figures">
          <div>
            <dt>{query.income ? t("Received") : t("Spent")}</dt>
            <dd>{money(shown, { symbol: true })}</dd>
          </div>
          <div>
            <dt>{t("Transactions")}</dt>
            <dd>{count}</dd>
          </div>
          <div>
            <dt>{t("Average")}</dt>
            <dd>{count ? money(Math.round(shown / count), { symbol: true }) : "—"}</dd>
          </div>
        </dl>
        {lines.length ? (
          <TransactionLines lines={lines} label={t("{title} transactions", { title })} showCategory maxHeight={440} />
        ) : (
          <p className="muted activity-empty">{t("No transactions.")}</p>
        )}
        <div className="modal-actions">
          <button className="secondary" onClick={onOpenRegister}>
            {t("Open in register")} <ArrowRight size={14} />
          </button>
          <button className="primary" onClick={onClose}>
            {t("Done")}
          </button>
        </div>
      </div>
    </Modal>
  );
}
