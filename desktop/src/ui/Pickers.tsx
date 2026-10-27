import { useMemo, type KeyboardEvent } from "react";
import { Combobox, type Option } from "./Combobox";
import { HIDDEN_GROUP, orderedGroups } from "../categories";
import { payeeIndex, type PayeeInfo } from "../payees";
import { money, tone } from "../format";
import type { Budget } from "../types";
import { t, useLanguage } from "../i18n";

/** Sentinel value for "Unassigned" (income, or no category). */
export const READY_TO_ASSIGN = "__rta";

/** Searchable category list grouped like the budget, with available balances when given. */
export function CategoryPicker({
  budget,
  value,
  onChange,
  available,
  readyToAssign = false,
  readyToAssignLast = false,
  includeHidden = false,
  includePayment = false,
  exclude,
  label = t("Category"),
  placeholder,
  autoFocus,
  className,
  onKeyDown,
  onCreate,
  onClear,
}: {
  budget: Pick<Budget, "categories" | "categoryOrder" | "groupOrder">;
  /** A category id, READY_TO_ASSIGN, or null for none chosen. */
  value: string | null;
  onChange: (id: string) => void;
  /** Available amount per category id, shown on the right. */
  available?: Map<string, number>;
  /** Offer "Unassigned" first; a string replaces its label. */
  readyToAssign?: boolean | string;
  /** List it last instead, under Other: spending already taken from Unassigned keeps showing as
   * that, after the categories it's meant for. */
  readyToAssignLast?: boolean;
  includeHidden?: boolean;
  /** Include credit card payment categories. */
  includePayment?: boolean;
  exclude?: string;
  label?: string;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
  /** Offers "Create category “…”" for a name no category has; gets the typed name. */
  onCreate?: (name: string) => void;
  /** Emptying the field leaves no category chosen (otherwise the last choice comes back). */
  onClear?: () => void;
}) {
  const language = useLanguage();
  const create = useMemo(
    () => (onCreate ? { label: t("Create category"), blank: t("New category…"), onCreate } : undefined),
    [onCreate, language],
  );
  const options = useMemo<Option[]>(() => {
    const unassigned: Option[] = readyToAssign
      ? [
          {
            id: READY_TO_ASSIGN,
            label: typeof readyToAssign === "string" ? readyToAssign : t("Unassigned"),
            group: readyToAssignLast ? t("Other") : t("Income"),
            keywords: "income inflow unassigned ready to assign",
          },
        ]
      : [];
    const list: Option[] = readyToAssignLast ? [] : unassigned;
    for (const group of orderedGroups(budget, { hidden: includeHidden }))
      for (const c of group.categories) {
        if ((!includePayment && c.paymentAccountId) || c.id === exclude) continue;
        const amount = available?.get(c.id);
        list.push({
          id: c.id,
          label: c.name,
          group: group.name === HIDDEN_GROUP ? t("Hidden") : group.name,
          detail: amount === undefined ? undefined : money(amount),
          detailTone: amount === undefined ? undefined : tone(amount),
        });
      }
    return readyToAssignLast ? [...list, ...unassigned] : list;
  }, [budget.categories, budget.categoryOrder, budget.groupOrder, available, readyToAssign, readyToAssignLast, includeHidden, includePayment, exclude, language]);
  return (
    <Combobox
      options={options}
      value={value}
      onChange={onChange}
      label={label}
      placeholder={placeholder ?? t("Choose a category")}
      autoFocus={autoFocus}
      className={className}
      onKeyDown={onKeyDown}
      create={create}
      onClear={onClear}
    />
  );
}

const TRANSFER = "\u0000transfer:";
/** How a transfer to or from another account reads in the payee field. */
export const transferPayee = (name: string) => t("Transfer: {name}", { name });

/**
 * Payee field with suggestions, most used first. `onPick` gets the payee's usual category.
 * With `transfers`, the other accounts are offered first ("Transfer: Savings"), as in YNAB.
 */
export function PayeeInput({
  budget,
  value,
  onChange,
  onPick,
  transfers,
  onTransfer,
  label = t("Payee"),
  placeholder = t("Payee"),
  autoFocus,
  className,
  onKeyDown,
}: {
  budget: Pick<Budget, "transactions" | "payees"> & Partial<Pick<Budget, "categories">>;
  value: string;
  onChange: (name: string) => void;
  onPick?: (payee: PayeeInfo) => void;
  /** Accounts that can be chosen as a transfer. */
  transfers?: { id: string; name: string }[];
  onTransfer?: (accountId: string) => void;
  label?: string;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
}) {
  const payees = useMemo(() => payeeIndex(budget), [budget.transactions, budget.payees, budget.categories]);
  // A name no payee has yet is kept as typed: the transaction makes it a payee once it's saved.
  const language = useLanguage();
  const create = useMemo(() => ({ label: t("New payee"), onCreate: () => {} }), [language]);
  const options = useMemo<Option[]>(
    () => [
      ...(transfers ?? []).map((a) => ({
        id: TRANSFER + a.id,
        label: transferPayee(a.name),
        group: t("Transfers"),
        keywords: "transfer move",
      })),
      ...payees.map((p) => ({ id: p.name, label: p.name, group: transfers?.length ? t("Payees") : undefined })),
    ],
    [payees, transfers, language],
  );
  return (
    <Combobox
      freeText
      options={options}
      text={value}
      onTextChange={onChange}
      onChange={(name) => {
        if (name.startsWith(TRANSFER)) return onTransfer?.(name.slice(TRANSFER.length));
        const info = payees.find((p) => p.name === name);
        if (info) onPick?.(info);
      }}
      label={label}
      placeholder={placeholder}
      autoFocus={autoFocus}
      className={className}
      onKeyDown={onKeyDown}
      groupHints={false}
      create={create}
    />
  );
}
