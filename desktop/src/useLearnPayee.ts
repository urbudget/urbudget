import { api } from "./bridge";
import { learnFromRename, ruleChanges } from "./bankNames";
import { errorText } from "./format";
import { useToast } from "./ui";
import { formatCount, t, tn } from "./i18n";
import { id, type Budget, type Transaction } from "./types";

type Transform = (fn: (b: Budget) => Promise<Budget>) => Promise<void>;

/**
 * Returns a function to call after a transaction is saved: when it came from a bank file and
 * was given a new payee, it offers to name that bank text the same way from now on, and then
 * to do the same to earlier transactions with it.
 */
export function useLearnPayee(transform: Transform, undo?: () => void) {
  const toast = useToast();
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
  const count = formatCount;
  return (before: Budget, saved: Transaction) => {
    const learned = learnFromRename(before, saved);
    if (!learned) return;
    const rule = { ...learned, id: id() };
    const add = async () => {
      let others = 0;
      await transform(async (b) => {
        const next = { ...b, payeeRules: [...(b.payeeRules ?? []), rule] };
        others = ruleChanges(next, new Set([rule.id]));
        return next;
      });
      const reapply = () =>
        transform((b) => api.action(b, { type: "reapplyPayeeRules", ruleIds: [rule.id] })).then(
          () =>
            toast({
              message: tn(others, "Updated {count} transaction.", "Updated {count} transactions.", { count: count(others) }),
              action: undo && { label: t("Undo"), onClick: undo },
            }),
          fail,
        );
      toast(
        others
          ? {
              message: tn(
                others,
                "Rule added. Apply it to {count} earlier transaction too?",
                "Rule added. Apply it to {count} earlier transactions too?",
                { count: count(others) },
              ),
              duration: 10000,
              action: { label: t("Apply"), onClick: () => void reapply() },
            }
          : { message: t("Rule added."), action: undo && { label: t("Undo"), onClick: undo } },
      );
    };
    toast({
      message: t("Always use {payee} for bank payees with “{match}”?", { payee: rule.payee, match: rule.match }),
      duration: 10000,
      action: { label: t("Add rule"), onClick: () => void add().catch(fail) },
    });
  };
}
