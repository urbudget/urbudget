import { useEffect, useRef, useState } from "react";
import { FolderOpen, Trash2 } from "lucide-react";
import { Modal } from "./forms";
import { api, type BudgetListing } from "./bridge";
import { BudgetFormatFields, type FormatValue } from "./BudgetFormatFields";
import { newBudget, type BudgetStart } from "./newBudget";
import { errorText, numberFormatFor } from "./format";
import { useToast } from "./ui";
import { localDate, t } from "./i18n";
import type { Budget, Summary } from "./types";
import { cloud } from "./cloud";
import { editorDrafts, discardEditorDraft, type EditorDraft } from "./editorCheckpoint";

/** Creates a budget file and opens it. */
export function NewBudgetDialog({
  current,
  close,
  open,
}: {
  current?: { budget: Budget; summary: Summary; token: string };
  close: () => void;
  /** Switches to the new budget (saving the open one first). */
  open: (id: string) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [localOnly, setLocalOnly] = useState(false);
  const [format, setFormat] = useState<FormatValue>({
    currency: current?.budget.currency ?? "SAR",
    numberFormat: numberFormatFor(current?.budget.currency ?? "SAR", current?.budget.numberFormat),
    dateFormat: current?.budget.dateFormat ?? "iso",
  });
  const [start, setStart] = useState<BudgetStart>("starter");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [created, setCreated] = useState<string | null>(null);
  const options: [BudgetStart, string, string][] = [
    ["starter", t("Starter categories"), t("Bills, needs, wants and savings groups to rename or delete.")],
    ["blank", t("Empty"), t("No accounts or categories.")],
    [
      "copy",
      t("Start over from “{budget}”", { budget: current?.budget.name ?? "" }),
      t("Keeps the same currency, balances, pending and future transactions, categories, targets, payees, rules and schedules. Settled history is removed; bank accounts need linking again."),
    ],
  ];
  return (
    <Modal title={t("New budget")} close={() => { if (!pending.current) close(); }}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (pending.current) return;
          if (!name.trim()) return setError(t("Give the budget a name."));
          pending.current = true;
          setBusy(true);
          setError("");
          try {
            const id = created ?? await api.createBudget(
              newBudget({ name, start, ...format }, current),
              start === "copy" && current ? { budget: current.budget, token: current.token } : undefined,
              localOnly,
            );
            setCreated(id);
            await open(id);
            close();
          } catch (e) {
            setError(errorText(e));
          } finally {
            pending.current = false;
            setBusy(false);
          }
        }}
      >
        <fieldset disabled={busy || created !== null} style={{ border: 0, margin: 0, padding: 0, display: "contents" }}>
        <label>
          {t("Budget name")}
          <input
            autoFocus
            required
            maxLength={200}
            placeholder={t("e.g. Household, Business, Travel 2027")}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label><input type="checkbox" checked={localOnly} onChange={e => setLocalOnly(e.target.checked)} />{t("Keep this budget only on this device")}</label>
        <BudgetFormatFields value={format} onChange={setFormat} />
        <fieldset className="choice-list">
          <legend>{t("Start with")}</legend>
          {options.filter(([value]) => value !== "copy" || current).map(([value, title, help]) => (
            <label key={value} className={`choice${start === value ? " selected" : ""}`}>
              <input
                type="radio"
                name="start"
                value={value}
                checked={start === value}
                onChange={() => setStart(value)}
              />
              <span>
                <strong>{title}</strong>
                <small>{help}</small>
              </span>
            </label>
          ))}
        </fieldset>
        </fieldset>
        {created && <p className="muted">{t("Your new budget was created. Retry opening it after resolving the error, or find it later in Budgets.")}</p>}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" disabled={busy} onClick={close}>
            {t("Cancel")}
          </button>
          <button className="primary" disabled={busy}>
            {busy ? (created ? t("Opening…") : t("Creating…")) : created ? t("Open created budget") : t("Create budget")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** One account library, including downloads and recoverable trash. */
export function ManageBudgetsDialog({ close, open, onNew, changed, review }: {
  close: () => void; open: (id: string) => Promise<void>; onNew?: () => void; changed?: () => Promise<void>; review?: (id: string) => Promise<void>;
}) {
  const toast = useToast();
  const [budgets, setBudgets] = useState<BudgetListing[] | null>(null);
  const [confirm, setConfirm] = useState<BudgetListing | null>(null);
  const [busy, setBusy] = useState(false);
  const [automaticLibrary, setAutomaticLibrary] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [drafts, setDrafts] = useState<EditorDraft[] | null>(null);
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
  useEffect(() => {
    let active = true;
    const refresh = () => api.listBudgets().then((rows) => { if (active) setBudgets(rows); }).catch(fail);
    void cloud.status().then(status => { if (active) setAutomaticLibrary(status.sync.protocol === 2); }).catch(fail);
    void refresh(); const timer = setInterval(() => void refresh(), 3000);
    return () => { active = false; clearInterval(timer); };
  }, [toast]);
  const action = async (kind: "trash" | "restore" | "evict" | "recover" | "review_deletion", entry: BudgetListing) => {
    if (busy) return;
    setBusy(true);
    try {
      if (kind === "review_deletion" && review) {
        await review(entry.id);
        setBudgets(await api.listBudgets());
      } else {
        const result = await api.libraryAction(kind, entry.id);
        setBudgets(result.library.entries);
      }
      setConfirm(null);
      await changed?.();
      if (kind === "review_deletion") close();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  return <Modal title={t("Budgets")} close={() => { if (!busy) close(); }}>
    {confirm ? <>
      <p>{confirm.scope === "account" ? t("Move this budget to Trash on all devices? You can restore it from Trash.") : t("Move this local budget to Trash? You can restore it later.")}</p>
      <strong>{confirm.name}</strong>
      <div className="modal-actions">
        <button className="secondary" disabled={busy} onClick={() => setConfirm(null)}>{t("Cancel")}</button>
        <button className="danger" disabled={busy} onClick={() => void action("trash", confirm)}>{t("Move to Trash")}</button>
      </div>
    </> : <>
      <ul className="budget-list">{(budgets ?? []).map((b) => {
        const trashed = b.lifecycle === "trashed" || b.lifecycle === "purged";
        const online = b.availability === "remote" || b.availability === "evicted";
        const deletionConflict = b.status === "deletion_conflict";
        return <li key={b.id} className={b.current ? "current" : ""}>
          <div>
            <strong>{b.name}</strong>
            <small>{deletionConflict ? t("Newer changes need review before deletion") : trashed ? t("In Trash") : b.lifecycle === "recovery" ? t("Recovered local changes") : b.pending ? t("Waiting to sync") : online ? t("Available online") : b.missing ? t("File not found") : b.scope === "local" ? t("Saved on this device") : b.current ? t("Open now") : b.lastOpened ? t("Opened {date}", { date: localDate(new Date(b.lastOpened)) }) : b.currency}</small>
            {automaticLibrary && b.scope === "local" && !trashed && <label><input type="checkbox" checked={selected.includes(b.id)} onChange={(e) => setSelected((ids) => e.target.checked ? [...ids, b.id] : ids.filter((id) => id !== b.id))} />{t("Add to account sync")}</label>}
          </div>
          <div className="row-actions">
            {!online && !b.missing && <button disabled={busy} onClick={() => void editorDrafts(b.id).then(setDrafts).catch(fail)}>{t("Recover unfinished edits")}</button>}
            {!trashed && b.lifecycle !== "recovery" && !b.current && (!b.missing || online) && <button className="secondary" disabled={busy} onClick={async () => { setBusy(true); try { await open(b.id); close(); } catch (e) { fail(e); } finally { setBusy(false); } }}>{t("Open")}</button>}
            {deletionConflict && <button disabled={busy} onClick={() => void action("review_deletion", b)}>{t("Review newer version")}</button>}
            {b.lifecycle === "trashed" && !deletionConflict && <button disabled={busy} onClick={() => void action("restore", b)}>{t("Restore")}</button>}
            {(b.lifecycle === "recovery" || (trashed && !b.missing)) && <button disabled={busy} onClick={() => void action("recover", b)}>{t("Recover as a copy")}</button>}
            {!trashed && b.scope === "account" && !online && <button className="secondary" disabled={busy || b.pending} onClick={() => void action("evict", b)}>{t("Remove download")}</button>}
            {b.path && !b.missing && !online && <button className="icon-button" title={t("Show in folder")} onClick={() => void api.reveal(b.path).catch(fail)}><FolderOpen size={15} /></button>}
            {!trashed && <button className="icon-button" disabled={busy} title={t("Move to Trash")} aria-label={t("Delete {budget}", { budget: b.name })} onClick={() => setConfirm(b)}><Trash2 size={15} /></button>}
          </div>
        </li>;
      })}{!budgets && <li>{t("Loading…")}</li>}{budgets?.length === 0 && <li>{t("Your library is empty.")}</li>}</ul>
      {selected.length > 0 && <button disabled={busy} onClick={async () => { setBusy(true); try { await api.associateBudgets(selected); setSelected([]); setBudgets(await api.listBudgets()); } catch (e) { fail(e); } finally { setBusy(false); } }}>{t("Add selected budgets to sync")}</button>}
      {drafts && <section><h3>{t("Unfinished edits")}</h3><p>{t("These fields were saved before the editor closed. Review them and re-enter the changes you want to keep.")}</p>{drafts.length === 0 && <p>{t("No unfinished edits.")}</p>}{drafts.map(draft => <div key={draft.editorId}><pre style={{whiteSpace:"pre-wrap"}}>{JSON.stringify(draft.draft, null, 2)}</pre><button onClick={async () => { try { await discardEditorDraft(draft); setDrafts(rows => rows?.filter(row => row.editorId !== draft.editorId) ?? null); } catch(e) { fail(e); } }}>{t("Discard these edits")}</button></div>)}</section>}
      <div className="modal-actions"><button className="secondary" disabled={busy} onClick={close}>{t("Close")}</button>{onNew && <button className="primary" disabled={busy} onClick={onNew}>{t("New budget…")}</button>}</div>
    </>}
  </Modal>;
}
