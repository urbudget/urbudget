// Settings › Account & sync: the changes two devices made differently, record by record, for
// the person to choose between. Everything else was already combined by sync (sync_merge.rs).
import { useState } from "react";
import { LoaderCircle } from "lucide-react";
import type { BudgetSync, SyncConflict } from "./cloud";
import { errorText, formatDate, money, monthName } from "./format";
import { Modal } from "./forms";
import { useToast } from "./ui";
import { list, msg, msgc, t, tn } from "./i18n";

export type Side = "local" | "cloud";

const KINDS: Record<SyncConflict["kind"], string> = {
  account: msg("Account"),
  category: msg("Category"),
  transaction: msg("Transaction"),
  assignment: msg("Assigned"),
  schedule: msg("Scheduled"),
  payee: msg("Payee"),
  payeeRule: msg("Bank name"),
  monthNote: msg("Month note"),
  loan: msg("Loan"),
  budget: msg("Budget"),
  setting: msg("Setting"),
};
const FIELDS: Record<string, string> = {
  name: msg("Name"),
  group: msg("Group"),
  target: msg("Target"),
  goal: msg("Target"),
  note: msg("Note"),
  hidden: msg("Hidden"),
  memo: msg("Memo"),
  payee: msg("Payee"),
  date: msg("Date"),
  accountId: msg("Account"),
  amount: msg("Amount"),
  status: msg("Cleared"),
  flag: msg("Flag"),
  approved: msg("Approved"),
  importedPayee: msg("Bank's payee name"),
  openingBalance: msg("Starting balance"),
  archived: msgc("account state", "Closed"),
  kind: msg("Type"),
  accountType: msg("Account type"),
  onBudget: msg("On budget"),
  netWorth: msg("In net worth"),
  nextDate: msg("Next date"),
  anchorDate: msg("First date"),
  frequency: msg("Repeats"),
  paused: msg("Paused"),
  transaction: msg("Transaction"),
  match: msg("Matches"),
  categoryId: msg("Category"),
  paymentAccountId: msg("Card"),
  currency: msg("Currency"),
  numberFormat: msg("Number format"),
  dateFormat: msg("Date format"),
  categoryOrder: msg("Category order"),
  groupOrder: msg("Group order"),
};
const MONEY = new Set(["target", "openingBalance", "monthlyPayment", "monthlyFees"]);
const DATES = new Set(["date", "nextDate", "anchorDate"]);
// An account's "Open" and "Closed" are states here, which Arabic words apart from the "Open"
// command, so their keys have a context.
const BOOLEANS: Record<string, [string, string]> = {
  approved: [msg("Approved"), msg("Not approved")],
  hidden: [msg("Hidden"), msg("Shown")],
  archived: [msgc("account state", "Closed"), msgc("account state", "Open")],
  paused: [msg("Paused"), msg("Active")],
  onBudget: [msg("On budget"), msg("Off budget")],
  netWorth: [msg("Counted"), msg("Not counted")],
};
const WORDS: Record<string, string> = {
  uncleared: msg("Uncleared"),
  cleared: msg("Cleared"),
  reconciled: msg("Reconciled"),
  cash: msg("Cash"),
  credit: msg("Credit card"),
  tracking: msg("Tracking"),
};

const fieldLabel = (field: string) =>
  FIELDS[field] ? t(FIELDS[field]) : field.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/^./, (c) => c.toUpperCase());

type Money = {
  amount?: number | null;
  lines?: { category: string | null; transfer: string | null; amount: number }[];
  transfer?: string | null;
  payee?: string;
  account?: string;
};
/** A transaction's money: the amount, then where it went. */
function moneyLine(m: Money) {
  const where = (l: { category: string | null; transfer: string | null }) =>
    l.transfer ? t("Transfer: {account}", { account: l.transfer }) : (l.category ?? t("Ready to Assign"));
  const lines = m.lines ?? [];
  const to = m.transfer
    ? t("Transfer: {account}", { account: m.transfer })
    : lines.length > 1
      ? list(lines.map((l) => `${where(l)} ${money(l.amount)}`))
      : lines[0]
        ? where(lines[0])
        : "";
  return [typeof m.amount === "number" ? money(m.amount) : "", to].filter(Boolean).join(" · ");
}

/** One side's version, as the person reads it. */
export function shownValue(c: SyncConflict, v: unknown): string {
  if (!c.field) {
    const changed = (v as { changed?: string[] } | null)?.changed ?? [];
    return v == null ? t("Deleted") : changed.length ? t("Edited: {fields}", { fields: list(changed.map(fieldLabel)) }) : t("Edited");
  }
  if (v == null) return c.field === "flag" ? t("No flag") : t("None");
  if (c.kind === "transaction" && c.field === "amount") return moneyLine(v as Money);
  if (c.kind === "schedule" && c.field === "transaction") {
    const m = v as Money;
    return [m.payee, moneyLine(m), m.account].filter(Boolean).join(" · ");
  }
  if (typeof v === "number") return MONEY.has(c.field) || c.kind === "assignment" ? money(v) : String(v);
  if (typeof v === "boolean") {
    const named = BOOLEANS[c.field]?.[v ? 0 : 1];
    return named ? t(named) : v ? t("Yes") : t("No");
  }
  if (typeof v === "string") {
    if (DATES.has(c.field) && /^\d{4}-\d{2}-\d{2}$/.test(v)) return formatDate(v);
    // A flag's colour, as the register names it.
    if (c.field === "flag") return t(v.charAt(0).toUpperCase() + v.slice(1));
    return WORDS[v] && ["status", "kind"].includes(c.field) ? t(WORDS[v]) : v === "" ? t("(empty)") : v;
  }
  if (Array.isArray(v))
    return v.every((x) => typeof x === "string") ? list(v as string[]) : tn(v.length, "{count} items", "{count} items");
  const flat = Object.entries(v as Record<string, unknown>).filter(([, x]) => x === null || typeof x !== "object");
  return flat.length ? flat.map(([k, x]) => `${fieldLabel(k)}: ${MONEY.has(k) || k === "amount" ? money(Number(x)) : String(x)}`).join(" · ") : t("Edited");
}

/** The record's name; a setting by what it's called, a month note by its month. */
function recordName(c: SyncConflict) {
  if (c.kind === "setting") return fieldLabel(c.label);
  if (c.kind === "monthNote" && /^\d{4}-\d{2}/.test(c.label)) return monthName(c.label.slice(0, 7));
  return c.label || t("(no name)");
}

/** What else tells the record apart. */
export function aboutText(c: SyncConflict): string {
  const a = c.about as { date?: string | null; account?: string; amount?: number | null; month?: string; group?: string; match?: string };
  switch (c.kind) {
    case "transaction":
    case "schedule":
      return [a.date ? (c.kind === "schedule" ? t("Next {date}", { date: formatDate(a.date) }) : formatDate(a.date)) : "", a.account, typeof a.amount === "number" ? money(a.amount) : ""]
        .filter(Boolean)
        .join(" · ");
    case "assignment":
      return a.month ? monthName(a.month) : "";
    case "category":
      return a.group ?? "";
    case "payeeRule":
      return a.match ? t("Matches “{text}”", { text: a.match }) : "";
    default:
      return "";
  }
}

export function SyncConflicts({
  budget,
  close,
  resolve,
}: {
  budget: BudgetSync;
  close: () => void;
  /** Merges again with these answers. */
  resolve: (choices: Record<string, Side>) => Promise<void>;
}) {
  const toast = useToast();
  const conflicts = budget.conflicts ?? [];
  const [choices, setChoices] = useState<Record<string, Side>>({});
  const [busy, setBusy] = useState(false);
  const here = t("This computer");
  const there = budget.device ?? t("The cloud copy");
  const shown = conflicts.slice(0, 100);
  const all = (side: Side) => setChoices(Object.fromEntries(conflicts.map((c) => [c.key, side])));
  const answered = conflicts.every((c) => choices[c.key]);
  return (
    <Modal title={t("Changed in two places: {budget}", { budget: budget.name })} close={close} wide>
      <p className="sync-conflicts-lead">
        {budget.device != null
          ? t("Everything else from {device} and this computer was combined.", { device: budget.device })
          : t("Everything else from your other device and this computer was combined.")}{" "}
        {tn(
          conflicts.length,
          "This was changed differently on both: choose which to keep.",
          "These {count} were changed differently on both: choose which to keep.",
        )}
      </p>
      {conflicts.length > 1 && (
        <div className="sync-conflicts-all">
          <span className="muted">{t("Keep all:")}</span>
          <button className="secondary" onClick={() => all("local")}>
            {t("{device}'s", { device: here })}
          </button>
          <button className="secondary" onClick={() => all("cloud")}>
            {t("{device}'s", { device: there })}
          </button>
        </div>
      )}
      <ul className="sync-conflicts">
        {shown.map((c) => {
          const about = aboutText(c);
          return (
            <li key={c.key} className="sync-conflict">
              <div className="sync-conflict-record">
                <strong>
                  {KINDS[c.kind] ? t(KINDS[c.kind]) : t("Record")}: {recordName(c)}
                </strong>
                {about && <small className="muted">{about}</small>}
              </div>
              <div className="sync-conflict-field">{c.field ? fieldLabel(c.field) : t("Deleted on one, edited on the other")}</div>
              <div className="sync-conflict-options" role="radiogroup" aria-label={`${t(KINDS[c.kind])} ${recordName(c)}: ${c.field ? fieldLabel(c.field) : t("keep or delete")}`}>
                {(["local", "cloud"] as const).map((side) => (
                  <label key={side} className={`sync-conflict-option${choices[c.key] === side ? " chosen" : ""}`}>
                    <input type="radio" name={c.key} checked={choices[c.key] === side} onChange={() => setChoices({ ...choices, [c.key]: side })} />
                    <span className="sync-conflict-side">{side === "local" ? here : there}</span>
                    <span className="sync-conflict-value">{shownValue(c, side === "local" ? c.local : c.remote)}</span>
                  </label>
                ))}
              </div>
            </li>
          );
        })}
      </ul>
      {conflicts.length > shown.length && (
        <p className="muted">
          {tn(
            conflicts.length - shown.length,
            "And {count} more. Keep all chooses for those too.",
            "And {count} more. Keep all chooses for those too.",
          )}
        </p>
      )}
      <div className="modal-actions">
        <button className="secondary" onClick={close} disabled={busy}>
          {t("Cancel")}
        </button>
        <button
          className="primary"
          disabled={busy || !answered}
          onClick={async () => {
            setBusy(true);
            try {
              await resolve(choices);
              close();
            } catch (e) {
              toast({ message: errorText(e), tone: "error" });
              setBusy(false);
            }
          }}
        >
          {busy && <LoaderCircle size={14} className="spin" />} {t("Keep these and sync")}
        </button>
      </div>
    </Modal>
  );
}
