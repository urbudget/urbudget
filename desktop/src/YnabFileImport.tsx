import { useRef, useState } from "react";
import { FileArchive } from "lucide-react";
import { api } from "./bridge";
import { errorText, money, numberFormatFor } from "./format";
import type { Budget } from "./types";
import { list, msg, t, tn } from "./i18n";

type Order = "" | "dmy" | "mdy" | "ymd";
type Difference = { month: string; category: string; field: string; expected: number; actual: number };
const FIELD: Record<string, string> = { budgeted: msg("assigned"), activity: msg("activity"), balance: msg("available") };
// Match ynab_files.rs. Check before reading: byte arrays sent over IPC take much more memory
// than the chosen files. Larger budgets can use the YNAB token import instead.
const MAX_FILE = 8 * 1024 * 1024;
const MAX_TOTAL = 16 * 1024 * 1024;
const MAX_FILES = 8;

/**
 * Makes a new budget from the export YNAB itself downloads (its .zip, or the Register and Plan
 * CSV files inside it). No token or connection is needed, and the open budget is left alone,
 * unless `onBudget` takes the imported budget instead (the welcome screen fills the empty one).
 */
export function YnabFileImport({
  budget,
  openBudget,
  onBudget,
}: {
  budget: Budget;
  /** Switches to the new budget (saving the open one first). */
  openBudget: (id: string) => Promise<void>;
  /** Takes the imported budget in place of making a new one. */
  onBudget?: (imported: Budget) => Promise<void>;
}) {
  const input = useRef<HTMLInputElement>(null);
  const createdId = useRef<string | null>(null);
  const [files, setFiles] = useState<File[]>([]);
  const [order, setOrder] = useState<Order>("");
  const [preview, setPreview] = useState<Budget | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  // Keep the open budget's separators, dates and hidden-currency choice. Otherwise a different
  // currency uses its own usual position. The preview and the created budget share this format.
  const importedNumberFormat =
    preview && preview.currency !== budget.currency
      ? {
          ...numberFormatFor(preview.currency, budget.numberFormat),
          symbol: budget.numberFormat?.symbol === "none" ? "none" as const : numberFormatFor(preview.currency).symbol,
        }
      : budget.numberFormat;

  const read = async (chosen: File[], dateOrder: Order) => {
    if (!chosen.length) return;
    createdId.current = null;
    setBusy(true);
    setPreview(null);
    setMessage(t("Reading the export…"));
    try {
      if (chosen.length > MAX_FILES) throw new Error(t("Choose at most 8 export files at once."));
      const oversized = chosen.find((f) => f.size > MAX_FILE);
      if (oversized)
        throw new Error(t("{name} is larger than 8 MB. Use YNAB's token import for larger budgets.", { name: oversized.name }));
      if (chosen.reduce((total, f) => total + f.size, 0) > MAX_TOTAL)
        throw new Error(t("These export files total more than 16 MB. Use YNAB's token import for larger budgets."));
      const payload: { name: string; bytes: number[] }[] = [];
      for (const file of chosen) {
        payload.push({ name: file.name, bytes: Array.from(new Uint8Array(await file.arrayBuffer())) });
      }
      setPreview(await api.importYnabFiles(payload, dateOrder || null, budget.currency));
      setMessage("");
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const create = async () => {
    if (!preview || busy) return;
    setBusy(true);
    const imported = { ...preview, numberFormat: importedNumberFormat, dateFormat: budget.dateFormat };
    try {
      if (onBudget) return await onBudget(imported);
      // Opening may fail after creation succeeded; a retry must reuse that saved budget.
      createdId.current ??= await api.createBudget(imported);
      await openBudget(createdId.current);
      setMessage(t("Created “{name}” from YNAB's export and opened it.", { name: preview.name }));
      setPreview(null);
      setFiles([]);
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const differences = (preview?.importAudit?.categoryDifferences ?? []) as Difference[];
  const kinds = (kind: string) => preview?.accounts.filter((a) => a.kind === kind) ?? [];
  const tracking = kinds("tracking");
  const months = new Set(preview?.assignments.map((a) => a.month)).size;
  const checks = tn(preview?.importAudit?.categoryMonthChecks ?? 0, "{count} category months", "{count} category months");
  return (
    <section className="panel">
      <h2>{t("Import YNAB's export file")}</h2>
      <p className="help">
        {onBudget
          ? t("In YNAB on the web, open your plan's menu and export it. Choose the .zip it downloads, or the Register and Plan files inside. It fills this empty budget. No token or connection is needed.")
          : t("In YNAB on the web, open your plan's menu and export it. Choose the .zip it downloads, or the Register and Plan files inside. This makes a new budget; the open one isn't changed. No token or connection is needed.")}
      </p>
      <div className="form-grid">
        <label>
          {t("Date order in the file")}
          <select
            value={order}
            disabled={busy}
            onChange={(e) => {
              const next = e.target.value as Order;
              setOrder(next);
              void read(files, next);
            }}
          >
            <option value="">{t("Detect automatically")}</option>
            <option value="dmy">{t("Day first (31/12/2026)")}</option>
            <option value="mdy">{t("Month first (12/31/2026)")}</option>
            <option value="ymd">{t("Year first (2026-12-31)")}</option>
          </select>
        </label>
      </div>
      <input
        ref={input}
        type="file"
        accept=".zip,.csv,.tsv"
        multiple
        hidden
        disabled={busy}
        aria-label={t("YNAB export file")}
        onChange={(e) => {
          const chosen = Array.from(e.target.files ?? []);
          e.target.value = "";
          setFiles(chosen);
          void read(chosen, order);
        }}
      />
      <button className="secondary" disabled={busy} onClick={() => input.current?.click()}>
        <FileArchive size={15} /> {files.length ? t("Choose other files…") : t("Choose export file…")}
      </button>
      {files.length > 0 && <p className="help">{list(files.map((f) => f.name))}</p>}
      {message && (
        <p className="import-result" role="status">
          {message}
        </p>
      )}
      {preview && (
        <div className="import-result" role="status">
          <p>
            {t("{name}: {accounts} ({cash} budget, {credit} credit, {tracking} tracking), {categories}, {transactions} and {months} of assignments.", {
              name: <strong>{preview.name}</strong>,
              accounts: tn(preview.accounts.length, "{count} accounts", "{count} accounts"),
              cash: kinds("cash").length,
              credit: kinds("credit").length,
              tracking: tracking.length,
              categories: tn(preview.categories.length, "{count} categories", "{count} categories"),
              transactions: tn(preview.transactions.length, "{count} transactions", "{count} transactions"),
              months: tn(months, "{count} months", "{count} months"),
            })}
          </p>
          <p>
            {differences.length === 0
              ? t("Checked {checks} against YNAB's plan: every amount matches.", { checks })
              : t("Checked {checks} against YNAB's plan: {differences}", {
                  checks,
                  differences: tn(differences.length, "{count} amounts differ.", "{count} amounts differ."),
                })}
          </p>
          {differences.length > 0 && (
            <ul>
              {differences.slice(0, 5).map((d, i) => (
                <li key={i}>
                  {t("{category}, {month}: {field} {actual} here, {expected} in YNAB", {
                    category: d.category,
                    month: d.month,
                    field: FIELD[d.field] ? t(FIELD[d.field]) : d.field,
                    actual: money(d.actual, { currency: preview.currency, numberFormat: importedNumberFormat }),
                    expected: money(d.expected, { currency: preview.currency, numberFormat: importedNumberFormat }),
                  })}
                </li>
              ))}
              {differences.length > 5 && <li>{tn(differences.length - 5, "…and {count} more", "…and {count} more")}</li>}
            </ul>
          )}
          {tracking.length > 0 && (
            <p className="help">
              {t("Tracking accounts: {names}. An account is treated as tracking when none of its rows has a category; change it afterwards if that's wrong.", {
                names: list(tracking.map((a) => a.name)),
              })}
            </p>
          )}
          <p className="help">
            {t("YNAB's export leaves out targets, notes, split memos and which accounts are closed. It has only the next date of each scheduled transaction, so those aren't brought in.")}
          </p>
          <button className="primary" disabled={busy} onClick={() => void create()}>
            {onBudget ? t("Import “{name}”", { name: preview.name }) : t("Create “{name}” and open it", { name: preview.name })}
          </button>{" "}
          <button className="text-button" disabled={busy} onClick={() => setPreview(null)}>
            {t("Cancel")}
          </button>
        </div>
      )}
    </section>
  );
}
