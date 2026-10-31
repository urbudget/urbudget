import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  LoaderCircle,
  MessageSquareText,
  Redo2,
  RefreshCw,
  Settings2,
  TriangleAlert,
  Undo2,
} from "lucide-react";
import { useBudget, type SyncState } from "./useBudget";
import { api } from "./bridge";
import { apply as applyPatch, type Patch } from "./patch";
import { AssistantBoundary, AssistantPanel, useAssistantWidth, type Request as AssistantRequest } from "./assistant/Assistant";
import { assistant, type Card, type Proposal } from "./assistant/api";
import { LoanActionsContext, hasLoan, loanTerms, type LoanActions } from "./Planning";
import { LoanPlanner } from "./LoanPlanner";
import { LoanPaymentForm } from "./LoanPayment";
import { ATTACH_LIMIT, categorizeText, fixImportText, importHelpText, type Uncategorized } from "./BankImport";
import { ImportPage, type ImportState } from "./import/ImportPage";
import { applyToLatest } from "./import/model";
import { BankSync, BankSyncDetails, syncHelpText } from "./BankSync";
import { Payees } from "./PayeesPage";
import { Reports } from "./Reports";
import { BudgetPage } from "./BudgetPage";
import { SettingsPage, type Tab as SettingsTab } from "./SettingsPage";
import { useCloud } from "./cloud";
import { SyncChip } from "./CloudBadges";
import { SyncSettings } from "./CloudSettings";
import { useEditorCheckpoint, flushAllEditorDrafts } from "./editorCheckpoint";
import { useAutomaticUpdate, useUpdatedNotice, useUpdates } from "./updates";
import { useUsageCounts } from "./usage";
import { UpdateChip } from "./UpdateNotice";
import { TransactionsPage, saveTransaction, scheduledMessage } from "./Register";
import { Sidebar, useSidebarLayout, type Page } from "./Sidebar";
import { ManageBudgetsDialog, NewBudgetDialog } from "./BudgetDialogs";
import { SampleBanner, Welcome, FirstRunWelcome, isBlank, isUntouched, openSample } from "./Welcome";
import { localeDefaults } from "./locale";
import { AccountForm, CategoryForm, Modal, ReconcileForm, TransactionForm } from "./forms";
import { configureFormat, errorText, formatDate, money, monthName, shiftMonth } from "./format";
import { groupNames } from "./categories";
import { HeaderSlotElements, Menu, MonthPicker, useToast } from "./ui";
import { useTheme } from "./theme";
import { useLearnPayee } from "./useLearnPayee";
import { useSteady } from "./useSteady";
import { useCategoryCreator } from "./NewCategory";
import { CardPaymentForm } from "./CardPayment";
import { getPref, resetLayout, usePref } from "./prefs";
import { TitleBar, customTitleBar } from "./WindowControls";
import { keyLabel } from "./platform";
import { getDirection, list, msg, t, tn, useLanguage } from "./i18n";
import { id, today, type Account, type Budget, type Category, type Transaction } from "./types";
import type { RegisterFilter } from "./Register";
import "./shell.css";

type Dialog =
  | { type: "account"; account?: Account; focusLoan?: boolean }
  | { type: "loanPayment"; account: Account }
  | { type: "planner"; account: Account }
  | { type: "category"; category?: Category }
  | { type: "transaction"; transaction?: Transaction; isNew?: boolean; repeat?: string }
  | { type: "reconcile"; account: Account }
  | { type: "payment"; account: Account }
  | { type: "bankSync"; account?: Account }
  | { type: "bankSyncDetails"; account: Account }
  | { type: "newBudget" }
  | { type: "manageBudgets" }
  | {
      type: "confirm";
      title: string;
      text: string;
      action: () => void | Promise<void>;
      label: string;
    }
  | null;
const syncLabels: Record<SyncState, string> = {
  loading: msg("Opening…"),
  saved: msg("Saved"),
  pending: msg("Unsaved"),
  saving: msg("Saving…"),
  conflict: msg("Changed elsewhere"),
  error: msg("Couldn't save"),
};
const titles: Record<Page, string> = {
  budget: msg("Budget"),
  transactions: msg("Transactions"),
  // The old Loan planner page, which now opens a loan's register (App).
  loans: msg("Loan details"),
  reports: msg("Reports"),
  payees: msg("Payees & rules"),
  settings: msg("Settings & data"),
  import: msg("Import a bank file"),
};
const kindLabel = (a: Account) =>
  a.kind === "credit" ? t("Credit card") : a.kind === "tracking" ? t("Tracking account") : t("Cash account");

/** Recovery must remain reachable when a month or draft cannot be calculated. */
function CalculationRecovery({ month, currentMonth, changeMonth, model, exportDraft, openBudget, closeEditor }: {
  month: string; currentMonth: string; changeMonth: (month: string) => void;
  model: ReturnType<typeof useBudget>; exportDraft: () => Promise<void>;
  openBudget: (id: string) => Promise<void>; closeEditor: () => void;
}) {
  const [reload, setReload] = useState(false);
  const [budgets, setBudgets] = useState(false);
  const busy = !!model.operation || !!model.recovery;
  const navigate = (next: string) => { closeEditor(); changeMonth(next); };
  const fetch = async () => { closeEditor(); await model.fetchDatabase(); setReload(false); };
  return (
    <section className="calculation-recovery" aria-label={t("Calculation recovery")}>
      <div className="modal-actions">
        <button className="secondary" aria-label={t("Previous month")} onClick={() => navigate(shiftMonth(month, -1))}><ChevronLeft size={15} /></button>
        <MonthPicker value={month} onChange={navigate} text={monthName(month, "short")} label={t("Choose budget month")} />
        <button className="secondary" aria-label={t("Next month")} onClick={() => navigate(shiftMonth(month, 1))}><ChevronRight size={15} /></button>
        <button className="secondary" onClick={() => navigate(currentMonth)} disabled={month === currentMonth}>{t("Current month")}</button>
      </div>
      <div className="modal-actions">
        <button className="secondary" disabled={!model.canUndo || busy || model.sync === "conflict"} onClick={() => { closeEditor(); model.undo(); }}>{t("Undo")}</button>
        <button className="secondary" disabled={!model.canRedo || busy || model.sync === "conflict"} onClick={() => { closeEditor(); model.redo(); }}>{t("Redo")}</button>
        <button className="secondary" onClick={() => void exportDraft()}>{t("Export draft")}</button>
        <button className="secondary" disabled={busy} onClick={() => model.dirty() ? setReload(true) : void fetch()}>{t("Reload saved budget")}</button>
        <button className="secondary" disabled={busy} onClick={() => setBudgets(true)}>{t("Open another budget")}</button>
      </div>
      {reload && <Modal title={t("Reload from disk?")} close={() => setReload(false)}>
        <p>{t("This replaces your unsaved changes with the saved budget. Export them first if you want to keep both.")}</p>
        <div className="modal-actions">
          <button className="secondary" onClick={() => setReload(false)}>{t("Cancel")}</button>
          <button className="primary" onClick={() => void fetch()}>{t("Discard changes & reload")}</button>
        </div>
      </Modal>}
      {budgets && <ManageBudgetsDialog close={() => setBudgets(false)} open={openBudget} />}
    </section>
  );
}

/** A dialog the user cannot dismiss: it waits for an operation or a decision. */
function Blocking({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="modal-shade">
      <div className="modal" role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-heading">
          <h2>{title}</h2>
        </div>
        {children}
      </div>
    </div>
  );
}

function SyncStatus({
  sync,
  onSave,
  onReload,
  onSettings,
}: {
  sync: SyncState;
  onSave: () => void;
  onReload: () => void;
  onSettings: () => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        ref={button}
        className={`sync-status ${sync}`}
        aria-expanded={open}
        title={t("Saving status")}
        onClick={() => setOpen(!open)}
      >
        {sync === "saving" ? <LoaderCircle size={13} className="spin" /> : <span className="dot" />}
        <span className="header-label">{t(syncLabels[sync])}</span>
      </button>
      <Menu
        open={open}
        onClose={() => setOpen(false)}
        anchor={button}
        placement="bottom-end"
        label={t("Saving")}
        items={[
          { label: t("Save now"), icon: <Check size={14} />, hint: keyLabel("Ctrl+S"), disabled: sync === "conflict", onSelect: onSave },
          { label: t("Reload from disk"), icon: <RefreshCw size={14} />, onSelect: onReload },
          "separator",
          { label: t("Settings & data"), icon: <Settings2 size={14} />, onSelect: onSettings },
        ]}
      />
    </>
  );
}

export default function App() {
  // The whole window renders again in a newly chosen language, keeping everything open.
  useLanguage();
  const [month, setMonth] = useState(today().slice(0, 7));
  const [dialog, setDialog] = useState<Dialog>(null);
  const [planningEditor, setPlanningEditor] = useState(false);
  const [importing, setImporting] = useState<ImportState | null>(null);
  const toast = useToast();
  const model = useBudget(
    month,
    !!importing?.session || planningEditor ||
      (!!dialog && !["confirm", "newBudget", "manageBudgets", "planner"].includes(dialog.type)),
  );
  const { budget, summary: currentSummary, edit, sync, metadata, budgetId } = model;
  const summary = currentSummary ?? model.displaySummary;
  // What the window shows of a save in progress: nothing for a quick one (most take ~50 ms), so
  // adding a transaction doesn't flash a dialog, a spinner and greyed-out buttons.
  const operation = useSteady(model.operation, (o) => !!o);
  // Longer than the 400 ms autosave delay, so an ordinary edit doesn't blink "Unsaved".
  const shownSync = useSteady(sync, (s) => s === "saving" || s === "pending", 700);
  // Every edit recalculates the month, usually in a few ms, so "Updating totals" shows only when
  // that's slow. The page goes inert straight away either way, so nothing acts on stale totals.
  const calculating = useSteady(!currentSummary, (c) => c);
  const learn = useLearnPayee(model.transform, model.undo);
  // "Create category" in the transaction dialog's category field.
  const newCategory = useCategoryCreator(budget, edit);
  const cloud = useCloud({
    dirty: model.dirty,
    sync,
    operation: model.operation,
    token: metadata?.token ?? "",
    fetchDatabase: model.refreshDatabase,
    canAdoptIncoming: model.canAdoptIncoming,
    budgetId,
  });
  const flushEditor = useEditorCheckpoint(budgetId,
    dialog && ["account", "category", "transaction", "reconcile", "payment", "loanPayment", "planner"].includes(dialog.type) ? dialog.type : planningEditor ? "inline" : null,
    {revision: metadata?.token, record: dialog, month, assignments: budget?.assignments}, error => toast({ message: errorText(error), tone: "error" }), undefined,
    planningEditor ? "main input, main textarea, main select, [aria-modal=true] input, [aria-modal=true] textarea, [aria-modal=true] select" : undefined);
  const flushImport = useEditorCheckpoint(budgetId, importing ? "bankImport" : null, {revision: metadata?.token, account: importing?.account},
    error => toast({ message: errorText(error), tone: "error" }), importing ? {...importing, content: undefined, file: importing.file ? {name: importing.file.name, size: importing.file.size} : undefined} : undefined,
    ".import-page input, .import-page textarea, .import-page select");
  model.setExitDrain(async () => { await flushEditor(); await flushImport(); await flushAllEditorDrafts(); await cloud.syncNow(); });
  const updates = useUpdates();
  useUsageCounts();
  const startPage = (): Page => (getPref("startPage") === "transactions" ? "transactions" : "budget");
  const [page, setPage] = useState<Page>(startPage);
  // A bank file being imported is kept while someone looks elsewhere.
  /** The assistant has a message being written or sent (AssistantPanel's onWorking). */
  const assistantWorking = useRef(false);
  // The import as it is now, for the assistant's fixes that land after a change of the user's.
  const importingNow = useRef(importing);
  importingNow.current = importing;
  const openImport = (next: ImportState) => {
    // One import at a time: an unfinished one is where the next starts.
    if (!importing?.session || next.file) setImporting(next);
    setPage("import");
  };
  const [accountId, setAccountId] = useState("all");
  const [registerFilter, setRegisterFilter] = useState<RegisterFilter | undefined>();
  const layout = useSidebarLayout();
  // A computer's page header: slots for a page's own controls (HeaderSlot), after the title and
  // beside undo and redo.
  const [headerSlot, setHeaderSlot] = useState<HTMLElement | null>(null);
  const [headerActions, setHeaderActions] = useState<HTMLElement | null>(null);
  const headerSlots = useMemo(() => ({ title: headerSlot, actions: headerActions }), [headerSlot, headerActions]);
  const [theme, setTheme] = useTheme();
  const fail = (e: unknown, prefix = "") => toast({ message: prefix + errorText(e), tone: "error" });
  const [assistantOpen, setAssistantOpen] = useState(() => {
    try {
      return localStorage.getItem("lb:assistant:open") === "1";
    } catch {
      return false;
    }
  });
  const [assistantRequest, setAssistantRequest] = useState<AssistantRequest | null>(null);
  const [highlight, setHighlight] = useState<Card["highlight"] | null>(null);
  const [settingsTab, setSettingsTab] = useState<{ tab: SettingsTab; key: number } | null>(null);
  const openSettings = (tab: SettingsTab) => {
    setSettingsTab({ tab, key: Date.now() });
    setPage("settings");
  };
  const assistantWidth = useAssistantWidth();
  // The budget right after the assistant's last applied change, so its Undo only shows while it is the latest edit.
  const lastApplied = useRef<{ id: string; budget: Budget } | null>(null);
  const budgetNow = useRef(budget);
  budgetNow.current = budget;
  const showAssistant = (open: boolean) => {
    setAssistantOpen(open);
    try {
      localStorage.setItem("lb:assistant:open", open ? "1" : "0");
    } catch {
      /* not remembered */
    }
    if (!open) setHighlight(null);
  };
  /** Opens the assistant, optionally with a question, a selection or files ready to send. */
  const ask = (request: Omit<AssistantRequest, "key"> = {}) => {
    showAssistant(true);
    setAssistantRequest({ ...request, key: Date.now() });
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "j") {
        e.preventDefault();
        setAssistantOpen((open) => {
          try {
            localStorage.setItem("lb:assistant:open", open ? "0" : "1");
          } catch {
            /* not remembered */
          }
          if (open) setHighlight(null);
          return !open;
        });
      }
    };
    // A file dropped anywhere else in the window goes to the assistant.
    const over = (e: DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    };
    const drop = (e: DragEvent) => {
      if (e.defaultPrevented || !e.dataTransfer?.files.length) return;
      e.preventDefault();
      ask({ files: [...e.dataTransfer.files] });
    };
    window.addEventListener("keydown", key);
    window.addEventListener("dragover", over);
    window.addEventListener("drop", drop);
    return () => {
      window.removeEventListener("keydown", key);
      window.removeEventListener("dragover", over);
      window.removeEventListener("drop", drop);
    };
  }, []);
  const close = () => setDialog(null);
  const editCategory = useCallback((category: Category) => setDialog({ type: "category", category }), []);
  const balances = useMemo(() => new Map((summary?.accounts ?? []).map((a) => [a.id, a])), [summary?.accounts]);
  // This month's available amounts, shown beside categories when picking one.
  const available = useMemo(
    () => new Map((summary?.categories ?? []).map((c) => [c.id, c.available])),
    [summary?.categories],
  );
  // Real group names only: "Hidden categories" is a display group, never a category's group.
  const groups = useMemo(() => (budget ? groupNames(budget) : []), [budget?.categories, budget?.groupOrder]);
  // A first launch's empty budget starts in the system's currency and formats, not the built-in ones.
  useEffect(() => {
    if (budget && model.ready && isUntouched(budget)) model.settle((d) => void Object.assign(d, localeDefaults()));
  }, [budget, model.ready]);
  // A different budget starts on the start page (Settings › General).
  useEffect(() => {
    setPage(startPage());
    setImporting(null);
    setAccountId("all");
    setRegisterFilter(undefined);
    setMonth(today().slice(0, 7));
  }, [budgetId]);
  // Loans live on their accounts now; the old Loan planner page opens the first loan's register
  // (one with details first), else the budget.
  useEffect(() => {
    if (page !== "loans" || !budget) return;
    const open = budget.accounts.filter((a) => !a.archived);
    const loan =
      open.find((a) => loanTerms(budget, a.id)) ?? open.find((a) => hasLoan(budget, a, balances.get(a.id)?.balance));
    setRegisterFilter(undefined);
    if (loan) setAccountId(loan.id);
    setPage(loan ? "transactions" : "budget");
  }, [page, budget]);

  const confirm = (title: string, text: string, label: string, action: () => void | Promise<void>) =>
    setDialog({ type: "confirm", title, text, label, action });
  const exportDraft = async (value: Budget | null = budget, recovery = false) => {
    if (!value) return;
    try {
      const path = recovery ? await api.exportRecoveryDraft(value) : await api.export(value);
      if (!path) return;
      toast({
        message: recovery ? t("Recovery draft exported.") : t("Full backup exported."),
        action: { label: t("Show in folder"), onClick: () => void api.reveal(path).catch(fail) },
      });
    } catch (e) {
      fail(e);
    }
  };
  const exportAccount = (account: Account) => {
    if (!budget) return;
    void api
      .exportTransactions(budget, account.id, null)
      .then((path) => {
        if (path)
          toast({
            message: t("Exported {account}.", { account: account.name }),
            action: { label: t("Show in folder"), onClick: () => void api.reveal(path).catch(fail) },
          });
      })
      .catch((e) => fail(e));
  };
  const reload = () => {
    if (model.dirty())
      confirm(
        t("Reload from disk?"),
        t("This replaces your unsaved changes with the saved budget. Export them first if you want to keep both."),
        t("Discard changes & reload"),
        model.fetchDatabase,
      );
    else void model.fetchDatabase();
  };
  // Saves the budget (and makes the on-close backup) before the installer replaces the app.
  const installUpdate = (automatic = false) =>
    void updates
      .install(model.installUpdate, automatic)
      .catch((e) => fail(e, t("Couldn't install the update: ")));
  // Unless Settings › General says not to, a downloaded update installs by itself: a Store one
  // when the window closes, after the budget is saved,
  const [autoUpdate] = usePref("autoUpdate");
  const { channel: updateChannel, installOnExit } = updates;
  const automaticUpdate = autoUpdate && updates.automatic;
  useEffect(() => {
    model.setExitTask(automaticUpdate && updateChannel === "microsoftStore" ? installOnExit : null);
  });
  // or any once UrBudget has sat unused in the background with nothing to lose: no unsaved
  // change, no dialog or form open, no import under way (on its page or not) and nothing unsent
  // in the assistant, none of which a restart keeps.
  useAutomaticUpdate(
    automaticUpdate,
    () =>
      !model.operation &&
      !model.dirty() &&
      !cloud.syncing &&
      !dialog &&
      !planningEditor &&
      !(importing?.file || importing?.session) &&
      !assistantWorking.current &&
      !document.querySelector('[aria-modal="true"]') &&
      !document.activeElement?.matches("input, textarea, select, [contenteditable]"),
    () => installUpdate(true),
  );
  useUpdatedNotice((version) => toast({ message: t("UrBudget was updated to version {version}.", { version }) }));
  const switchBudget = (id: string) =>
    model.switchBudget(id).catch((e) => {
      fail(e, t("Couldn't open that budget: "));
      throw e;
    });
  const loadImport = async (file: File) => {
    try {
      const raw = JSON.parse(await file.text());
      const imported = raw.data?.plan || raw.data?.budget ? await api.importYnab(raw) : (raw as Budget);
      await api.validate(imported);
      confirm(
        t("Restore from this backup?"),
        t("Replace “{current}” with “{backup}” ({accounts}, {transactions})? A backup of the current budget is made first.", {
          current: String(budget?.name),
          backup: imported.name,
          accounts: tn(imported.accounts.length, "{count} accounts", "{count} accounts"),
          transactions: tn(imported.transactions.length, "{count} transactions", "{count} transactions"),
        }),
        t("Back up & restore"),
        () => model.restoreBackup(imported),
      );
    } catch (e) {
      fail(e, t("Couldn't read that file: "));
    }
  };

  if (model.ready && !budget) return (
    <div className="empty-library-page">
      {customTitleBar && <div className="empty-library-titlebar"><TitleBar /></div>}
      {cloud.status?.library?.firstRun ? <FirstRunWelcome openBudget={switchBudget} onAddAccount={() => setDialog({type:"account"})} importYnab={model.importYnab} acceptYnab={model.acceptYnab} resolveYnab={model.resolveYnab} /> : <div className="settings-page">
      <h1>{t("Budget library")}</h1>
      <p>{t("Create a budget or open one from your account library.")}</p>
      <button className="primary" onClick={() => setDialog({ type: "newBudget" })}>{t("New budget")}</button>{" "}
      <button className="secondary" onClick={() => setDialog({ type: "manageBudgets" })}>{t("Budgets")}</button>
      </div>}
      <div className="settings-page"><SyncSettings cloud={cloud} budgetId="" isClean={() => true} openBudget={switchBudget} /></div>
      {dialog?.type === "newBudget" && <NewBudgetDialog close={close} open={switchBudget} />}
      {dialog?.type === "manageBudgets" && <ManageBudgetsDialog close={close} open={switchBudget} changed={model.refreshDatabase} review={async (id) => { await flushAllEditorDrafts(); await model.switchBudget(id, true); }} onNew={() => setDialog({ type: "newBudget" })} />}
    </div>
  );
  if (!budget || !summary)
    return (
      <div className="loading-screen">
        {customTitleBar && (
          <div className="loading-titlebar">
            <TitleBar />
          </div>
        )}
        <h1>UrBudget</h1>
        <p>{model.calculationError ? t("Could not calculate {month}: {error}", { month: monthName(month), error: model.calculationError }) : model.error || (budget ? t("Updating {month} totals…", { month: monthName(month) }) : t("Opening your budget…"))}</p>
        {model.calculationError ? (
          <button className="primary" onClick={model.retryCalculation}>{t("Retry calculation")}</button>
        ) : model.error ? (
          <button className="primary" onClick={() => location.reload()}>
            {t("Try again")}
          </button>
        ) : (
          <LoaderCircle className="spin" />
        )}
        {budget && model.calculationError && <CalculationRecovery
          month={month} currentMonth={model.calculationMonth ?? today().slice(0, 7)} changeMonth={setMonth}
          model={model} exportDraft={() => exportDraft(budget, true)} openBudget={switchBudget} closeEditor={close}
        />}
      </div>
    );
  const formatKey = configureFormat(budget);
  const currentAccount = budget.accounts.find((a) => a.id === accountId);
  const navigateAccount = (account: string) => {
    setAccountId(account);
    setPage("transactions");
    setRegisterFilter(undefined);
  };
  const thisMonth = model.calculationMonth ?? today().slice(0, 7);
  const editCalculated: typeof edit = (update) => { if (currentSummary) edit(update); };
  const transformCalculated: typeof model.transform = (work) => currentSummary
    ? model.transform(work) : Promise.reject(new Error(t("Wait for the budget calculation to finish before changing these amounts.")));
  // A loan's actions, from its register, its payment category and its account menus. Without
  // details saved, each opens the account form at them.
  const loanActions: LoanActions = {
    record: (a) => { if (currentSummary) setDialog(loanTerms(budget, a.id) ? { type: "loanPayment", account: a } : { type: "account", account: a, focusLoan: true }); },
    plan: (a) => {
      if (!currentSummary) return;
      // Nothing to plan without details, or once it's paid off.
      const owed = -(balances.get(a.id)?.balance ?? a.openingBalance);
      if (!loanTerms(budget, a.id) || owed <= 0) return setDialog({ type: "account", account: a, focusLoan: true });
      if (page !== "transactions" || accountId !== a.id) navigateAccount(a.id);
      setDialog({ type: "planner", account: a });
    },
    details: (a) => setDialog({ type: "account", account: a, focusLoan: true }),
  };
  const dialogLoan = dialog && (dialog.type === "planner" || dialog.type === "loanPayment") ? loanTerms(budget, dialog.account.id) : undefined;

  /** Applies an assistant proposal as one undoable change; a refreshed card comes back when the budget moved. */
  /** Fixes the assistant proposed for the import in progress: applied there, where each still fits. */
  const applyFix = async (threadId: string, p: Proposal): Promise<Proposal> => {
    const fixes = p.fixes;
    const open = importing?.session && importing.content !== undefined ? importing : null;
    if (!fixes || !open?.session || open.content === undefined) throw new Error(t("Open the import these fixes are for."));
    if (fixes.scope.session !== open.session.scope.session) throw new Error(t("These fixes are for another import."));
    // Made on the import as it is when they land: a change of the user's meanwhile stays.
    const applied = await applyToLatest(
      () => importingNow.current?.session,
      (base) => api.bankApplyFixes(budget!, open.account, open.content!, base, fixes),
    );
    if (!applied) throw new Error(t("These fixes are for another import."));
    const lines = applied.results.filter((r) => r.applied && r.line).map((r) => r.line!);
    setImporting((was) =>
      was?.session === applied.base
        ? { ...was, session: applied.session, view: "editor", assisted: [...new Set([...(was.assisted ?? []), ...lines])] }
        : was,
    );
    setPage("import");
    const refused = applied.results.filter((r) => !r.applied);
    const done = applied.results.length - refused.length;
    toast({
      message: refused.length
        ? `${tn(applied.results.length, "Applied {done} of {count} fixes.", "Applied {done} of {count} fixes.", { done })} ${
            refused.length === 1 ? t("One no longer fits") : tn(refused.length, "{count} no longer fit", "{count} no longer fit")
          }: ${refused[0].problem ?? t("the line changed")}`
        : done === 1
          ? t("Applied the fix to the import.")
          : tn(done, "Applied {count} fix to the import.", "Applied {count} fixes to the import."),
      tone: refused.length && !done ? "error" : undefined,
    });
    return done ? assistant.mark(threadId, p.id, "applied") : p;
  };
  const applyProposal = async (threadId: string, p: Proposal): Promise<Proposal> => {
    if (p.kind === "fix") return applyFix(threadId, p);
    if (!currentSummary) throw new Error(t("Wait for the budget calculation to finish before applying these changes."));
    let outcome = null as { patch: Patch | null; proposal: Proposal } | null;
    let after: Budget | null = null;
    try {
      await model.transform(async (b) => {
        outcome = await assistant.apply(b, threadId, p.id);
        if (!outcome.patch) throw new Error("refreshed");
        after = applyPatch(b, outcome.patch);
        return after;
      });
    } catch (e) {
      if (outcome && !outcome.patch) return outcome.proposal;
      if (outcome?.patch) await assistant.mark(threadId, p.id, "proposed").catch(() => {});
      throw e;
    }
    if (after) lastApplied.current = { id: p.id, budget: after };
    toast({
      message: t("Applied: {proposal}", { proposal: p.title }),
      action: {
        label: t("Undo"),
        onClick: () => {
          // As the card's Undo: only while it's still the latest edit, or this would undo the one made since.
          if (lastApplied.current?.id === p.id && lastApplied.current.budget === budgetNow.current) void undoProposal(threadId, p);
          else toast({ message: t("Not undone: the budget has changed since it was applied.") });
        },
      },
    });
    return outcome!.proposal;
  };
  const undoProposal = async (threadId: string, p: Proposal): Promise<Proposal> => {
    if (lastApplied.current?.id === p.id) {
      model.undo();
      lastApplied.current = null;
    }
    return assistant.mark(threadId, p.id, "undone");
  };
  /** Opens the assistant with an import's uncategorized transactions attached and a request ready to send. */
  const categorize = (uncategorized: Uncategorized) =>
    ask({ text: categorizeText(uncategorized), selected: uncategorized.ids.slice(0, ATTACH_LIMIT) });
  const reviewImport = async (threadId: string, p: Proposal) => {
    try {
      const file = await assistant.importFile(threadId, p.id);
      const account = budget.accounts.find((a) => a.id === file.account);
      if (!account) throw new Error(t("That account no longer exists."));
      openImport({ account: account.id, file: new File([file.csv], file.name, { type: "text/csv" }), proposal: { threadId, id: p.id } });
    } catch (e) {
      fail(e);
    }
  };
  const highlightStyle = (() => {
    if (!assistantOpen || !highlight) return "";
    const esc = (v: string) => (typeof CSS !== "undefined" && CSS.escape ? CSS.escape(v) : v.replace(/["\\]/g, "\\$&"));
    const selectors = [
      ...(page === "budget" && (!highlight.month || highlight.month === month)
        ? highlight.categories.map((c) => `[data-category="${esc(c)}"] > *`)
        : []),
      ...(page === "transactions" ? highlight.transactions.map((t) => `[id="reg-${esc(t)}"]`) : []),
    ];
    return selectors.length
      ? `${selectors.join(",")}{background:var(--accent-soft)!important;box-shadow:inset ${getDirection() === "rtl" ? -3 : 3}px 0 0 var(--accent)}`
      : "";
  })();

  const title =
    page === "budget" ? (
      <div className="month-nav">
        <button className="icon-button" aria-label={t("Previous month")} onClick={() => setMonth(shiftMonth(month, -1))}>
          <ChevronLeft size={16} />
        </button>
        <h1>
          <span className="month-long">{monthName(month)}</span>
          <span className="month-short">{monthName(month, "short")}</span>
        </h1>
        <button className="icon-button" aria-label={t("Next month")} onClick={() => setMonth(shiftMonth(month, 1))}>
          <ChevronRight size={16} />
        </button>
        <MonthPicker value={month} onChange={setMonth} label={t("Choose budget month")} />
        {month !== thisMonth && (
          <button className="link-button" onClick={() => setMonth(thisMonth)}>
            {t("Today")}
          </button>
        )}
      </div>
    ) : page === "transactions" ? (
      <div className="title-stack">
        <h1>{currentAccount?.name ?? t("All transactions")}</h1>
        <span>
          {currentAccount
            ? currentAccount.archived
              ? t("{kind} · Closed", { kind: kindLabel(currentAccount) })
              : kindLabel(currentAccount)
            : tn(budget.accounts.length, "{count} accounts", "{count} accounts")}
        </span>
      </div>
    ) : (
      <h1>{t(titles[page])}</h1>
    );

  // On Windows these sit in the title bar beside the window buttons; elsewhere, in the header.
  const saveStatus = (
    <SyncStatus sync={shownSync} onSave={() => void model.flush()} onReload={reload} onSettings={() => setPage("settings")} />
  );
  const assistantButton = (
    <button
      className="assistant-toggle"
      aria-pressed={assistantOpen}
      title={keyLabel(t("Assistant (Ctrl+J)"))}
      onClick={() => showAssistant(!assistantOpen)}
    >
      <MessageSquareText size={15} />
      <span className="header-label">{t("Assistant")}</span>
    </button>
  );

  const cloudChips = (
    <>
      <UpdateChip updates={updates} onOpen={() => openSettings("general")} />
      <SyncChip cloud={cloud} onOpen={() => openSettings("sync")} />
    </>
  );

  return (
    <HeaderSlotElements.Provider value={headerSlots}>
    <LoanActionsContext.Provider value={loanActions}>
    <div
      className={`app-shell${assistantOpen ? " with-assistant" : ""}${customTitleBar ? " has-titlebar" : ""}`}
      style={{ "--sidebar-width": `${layout.width}px`, "--assistant-width": `${assistantWidth.width}px` } as CSSProperties}
    >
      {customTitleBar && (
        <TitleBar>
          {cloudChips}
          {saveStatus}
          {assistantButton}
        </TitleBar>
      )}
      {highlightStyle && <style>{highlightStyle}</style>}
      <Sidebar
        budget={budget}
        budgetId={budgetId}
        balances={balances}
        page={page}
        accountId={accountId}
        layout={layout}
        edit={edit}
        onNavigate={setPage}
        importing={importing ? (budget.accounts.find((a) => a.id === importing.account)?.name ?? "") : undefined}
        onAccount={navigateAccount}
        onAddAccount={() => setDialog({ type: "account" })}
        onEditAccount={(account) => setDialog({ type: "account", account })}
        onReconcile={(account) => { if (currentSummary) setDialog({ type: "reconcile", account }); }}
        onExportAccount={exportAccount}
        onSync={(account) => setDialog({ type: "bankSync", account })}
        onSyncDetails={(account) => setDialog({ type: "bankSyncDetails", account })}
        onLoan={loanActions.plan}
        onSwitchBudget={(id) => void switchBudget(id).catch(() => {})}
        onNewBudget={() => setDialog({ type: "newBudget" })}
        onManageBudgets={() => setDialog({ type: "manageBudgets" })}
      />

      <main className="main-area">
        <header className="app-header">
          <div className="header-title">{title}</div>
          <div className="header-slot" ref={setHeaderSlot} />
          <div className="header-actions">
            {!customTitleBar && cloudChips}
            {!customTitleBar && saveStatus}
            <div className="header-page-actions" ref={setHeaderActions} />
            <button
              className="icon-button"
              aria-label={t("Undo")}
              title={keyLabel(t("Undo (Ctrl+Z)"))}
              disabled={!model.canUndo || !!operation || sync === "conflict"}
              onClick={model.undo}
            >
              <Undo2 size={16} />
            </button>
            <button
              className="icon-button"
              aria-label={t("Redo")}
              title={keyLabel(t("Redo (Ctrl+Shift+Z)"))}
              disabled={!model.canRedo || !!operation || sync === "conflict"}
              onClick={model.redo}
            >
              <Redo2 size={16} />
            </button>
            {!customTitleBar && assistantButton}
          </div>
        </header>
        <SampleBanner
          budget={budget}
          budgetId={budgetId}
          openBudget={switchBudget}
          onNewBudget={() => setDialog({ type: "newBudget" })}
        />
        {(sync === "conflict" || model.error) && (
          <div className="alert" role="alert">
            <TriangleAlert size={16} />
            <span>{model.error}</span>
            {sync === "conflict" ? (
              <>
                <button className="secondary" onClick={() => void exportDraft()}>
                  {t("Export unsaved changes")}
                </button>
                <button className="secondary" onClick={reload}>
                  {t("Reload from disk")}
                </button>
              </>
            ) : sync === "error" ? (
              <button className="secondary" onClick={() => void model.flush()}>
                {t("Retry")}
              </button>
            ) : (
              <button className="secondary" onClick={model.dismissError}>
                {t("Dismiss")}
              </button>
            )}
          </div>
        )}
        {calculating && (
          <div className="calculating" role="status"><span><LoaderCircle size={12} className="spin" />{t("Updating {month} totals…", { month: monthName(month) })}</span></div>
        )}
        <div key={formatKey} inert={!currentSummary && ["budget", "transactions"].includes(page)} aria-busy={!currentSummary} className={`page-scroll${page === "transactions" || page === "budget" || page === "reports" ? " fill" : ""}`}>
          {page === "budget" && isBlank(budget) && (
            <Welcome
              key={budgetId}
              budget={budget}
              budgetId={budgetId}
              edit={edit}
              transform={model.transform}
              openBudget={switchBudget}
              onAddAccount={() => setDialog({ type: "account" })}
              importYnab={model.importYnab}
              acceptYnab={model.acceptYnab}
              resolveYnab={model.resolveYnab}
              onAsk={(text) => ask({ text })}
            />
          )}
          {page === "budget" && !isBlank(budget) && (
            <BudgetPage
              budget={budget}
              budgetId={budgetId}
              undo={model.undo}
              summary={summary}
              month={month}
              edit={editCalculated}
              transform={transformCalculated}
              onEditorChange={setPlanningEditor}
              onEditCategory={editCategory}
              onAsk={(names) =>
                ask({ text: t("Help me with {categories} this month.", { categories: names.length === 1 ? names[0] : list(names) }) })
              }
              onNewCategory={() => setDialog({ type: "category" })}
              onAddAccount={() => setDialog({ type: "account" })}
              onOpenRegister={({ accountId: account, ...filter }) => {
                setAccountId(account);
                setRegisterFilter(filter);
                setPage("transactions");
              }}
              onOpenSample={() => void openSample(budget, budgetId, switchBudget).catch((e) => fail(e))}
              onAddTransaction={() => setDialog({ type: "transaction", isNew: true })}
              onPayCard={(id) => {
                const card = budget.accounts.find((a) => a.id === id);
                if (card) setDialog({ type: "payment", account: card });
              }}
            />
          )}
          {page === "transactions" && (
            <TransactionsPage
              budget={budget}
              accountId={accountId}
              account={currentAccount}
              summary={balances.get(accountId)}
              netWorth={summary.netWorth}
              edit={editCalculated}
              transform={transformCalculated}
              initialFilter={registerFilter}
              available={available}
              undo={model.undo}
              onEditorChange={setPlanningEditor}
              onAsk={(ids) => ask({ selected: ids })}
              onEditAccount={(account) => setDialog({ type: "account", account })}
              onReconcile={(account) => setDialog({ type: "reconcile", account })}
              onPayCard={(account) => setDialog({ type: "payment", account })}
              onOpenTransfer={(account, id) => {
                setAccountId(account);
                setRegisterFilter({ focus: id });
              }}
              onImport={(account) => openImport({ account: account.id })}
              onSync={(account) => setDialog({ type: "bankSync", account })}
              onEdit={(transaction) => setDialog({ type: "transaction", transaction })}
              onAddDialog={(transaction, repeat) => setDialog({ type: "transaction", transaction, isNew: true, repeat })}
            />
          )}
          {page === "reports" && (
            <Reports
              budget={budget}
              budgetId={budgetId}
              month={month}
              onInspect={(filter) => {
                setAccountId("all");
                setRegisterFilter(filter);
                setPage("transactions");
              }}
            />
          )}
          {page === "payees" && <Payees budgetId={budgetId} budget={budget} edit={edit} transform={model.transform} undo={model.undo} />}
          {page === "import" && importing && (
            <ImportPage
              budget={budget}
              budgetId={budgetId}
              state={importing}
              onState={(next) => setImporting((was) => (typeof next === "function" ? (was ? next(was) : was) : next))}
              transform={model.transform}
              onImported={(count, uncategorized) => {
                const from = importing.proposal;
                if (from) void assistant.mark(from.threadId, from.id, "applied").catch(() => {});
                setImporting(null);
                setAccountId(importing.account);
                setRegisterFilter(undefined);
                setPage("transactions");
                toast({
                  message: tn(count, "Imported {count} transaction for review.", "Imported {count} transactions for review."),
                  action: { label: t("Undo"), onClick: model.undo },
                  offer: uncategorized.ids.length ? { label: t("Categorize with the assistant"), onClick: () => categorize(uncategorized) } : undefined,
                });
              }}
              onCancel={() => {
                setImporting(null);
                setPage("transactions");
              }}
              onAskAssistant={
                // A file the assistant already converted goes back to it from its own card instead.
                importing.proposal ? undefined : (file, problem, account) => ask({ text: importHelpText(account, problem), files: [file] })
              }
              onFixWithAssistant={() => ask({ text: fixImportText() })}
            />
          )}
          {page === "settings" && (
            <SettingsPage
              key={settingsTab?.key}
              initialTab={settingsTab?.tab}
              budget={budget}
              sync={sync}
              syncLabel={t(syncLabels[sync])}
              metadata={metadata}
              edit={edit}
              flush={model.flush}
              onReload={reload}
              onExport={() => void exportDraft()}
              onImportFile={(file) => void loadImport(file)}
              importYnab={model.importYnab}
              acceptYnab={model.acceptYnab}
              resolveYnab={model.resolveYnab}
              onAsk={(text) => ask({ text })}
              openBudget={switchBudget}
              onBankSync={() => setDialog({ type: "bankSync" })}
              onBankImport={() => openImport({ account: accountId !== "all" && budget.accounts.some((a) => a.id === accountId && !a.archived) ? accountId : (budget.accounts.find((a) => !a.archived)?.id ?? "") })}
              theme={theme}
              setTheme={setTheme}
              cloud={cloud}
              updates={updates}
              onInstallUpdate={() => installUpdate()}
              budgetId={budgetId}
              isClean={() => !model.dirty() && sync === "saved"}
              onResetLayout={() => {
                resetLayout();
                layout.setWidth(236);
                if (layout.collapsed) layout.toggle();
              }}
            />
          )}
        </div>
      </main>
      {assistantOpen && (
        <AssistantBoundary budgetId={budgetId} onClose={() => showAssistant(false)}>
          <AssistantPanel
            onWorking={(working) => {
              assistantWorking.current = working;
            }}
            budget={budget}
            budgetId={budgetId}
            summary={summary}
            screen={{
              page,
              month,
              accountId: page === "transactions" ? accountId : null,
              // The bank file on the Import page: the assistant is told its lines, as they read when a message is sent.
              import:
                page === "import" && importing?.session && importing.content !== undefined
                  ? { account: importing.account, content: importing.content, session: importing.session }
                  : undefined,
            }}
            request={assistantRequest}
            width={assistantWidth.width}
            onResize={assistantWidth.setWidth}
            onClose={() => showAssistant(false)}
            onOpenSettings={() => openSettings("assistant")}
            onApply={applyProposal}
            onUndo={undoProposal}
            canUndo={(p) => lastApplied.current?.id === p.id && lastApplied.current.budget === budget}
            onReviewImport={(threadId, p) => void reviewImport(threadId, p)}
            onHighlight={setHighlight}
          />
        </AssistantBoundary>
      )}

      {/* Clicks wait while anything is saving; the dialog shows only if it takes a while. */}
      {model.operation && !operation && updates.phase !== "installing" && <div className="operation-guard" aria-hidden />}
      {operation && updates.phase !== "installing" && (
        <Blocking title={t(operation)}>
          <p role="status">{t("Please keep this window open; this only takes a moment.")}</p>
          <LoaderCircle className="spin" />
        </Blocking>
      )}
      {updates.phase === "installing" && (
        <Blocking title={t("Installing the update")}>
          <p role="status">{updates.channel === "microsoftStore"
            ? t("Your budget is saved first, then Windows installs the update, which closes UrBudget and opens it again. Windows may ask for permission first.")
            : t("Your budget is saved first, then UrBudget restarts on the new version.")}</p>
          <LoaderCircle className="spin" />
        </Blocking>
      )}
      {model.recovery && (
        <Blocking title={t("Restore unsaved changes?")}>
          <p>
            {t("UrBudget closed before some changes were saved. Restore them to continue where you left off, or keep the saved budget.")}
          </p>
          <div className="modal-actions">
            <button className="secondary" onClick={() => void exportDraft(model.recovery!.budget)}>
              {t("Export them")}
            </button>
            <button className="secondary" onClick={() => void model.fetchDatabase()}>
              {t("Keep saved budget")}
            </button>
            <button className="primary" onClick={() => void model.restoreRecovery().catch((e) => fail(e))}>
              {t("Restore changes")}
            </button>
          </div>
        </Blocking>
      )}
      {currentSummary && dialog?.type === "newBudget" && (
        <NewBudgetDialog current={{ budget, summary, token: metadata?.token ?? "" }} close={close} open={switchBudget} />
      )}
      {dialog?.type === "manageBudgets" && (
        <ManageBudgetsDialog
          close={close}
          open={switchBudget}
          changed={model.refreshDatabase} review={async (id) => { await flushAllEditorDrafts(); await model.switchBudget(id, true); }}
          onNew={() => setTimeout(() => setDialog({ type: "newBudget" }))}
        />
      )}
      {dialog?.type === "account" && (
        <AccountForm
          initial={dialog.account}
          balance={dialog.account && balances.get(dialog.account.id)?.balance}
          budget={budget}
          focusLoan={dialog.focusLoan}
          close={close}
          save={(account, loan) => {
            edit((d) => {
              const index = d.accounts.findIndex((a) => a.id === account.id);
              if (index < 0) d.accounts.push(account);
              else d.accounts[index] = account;
              // A loan's details, by account (null removes them).
              if (loan !== undefined && (loan || d.loans?.some((l) => l.accountId === account.id)))
                d.loans = [...(d.loans ?? []).filter((l) => l.accountId !== account.id), ...(loan ? [loan] : [])];
              if (account.kind !== "credit" || account.onBudget === false) return;
              // A card's payment category follows the card's name, as in YNAB.
              const payment = d.categories.find((c) => c.paymentAccountId === account.id);
              if (payment) payment.name = account.name;
              else
                d.categories.push({
                  id: id(),
                  name: account.name,
                  group: "Credit Card Payments",
                  target: 0,
                  paymentAccountId: account.id,
                });
            });
            close();
          }}
        />
      )}
      {dialog?.type === "category" && (
        <CategoryForm
          initial={dialog.category}
          groups={groups}
          month={month}
          currentMonth={thisMonth}
          close={close}
          save={(category) => {
            edit((d) => {
              const index = d.categories.findIndex((c) => c.id === category.id);
              if (index < 0) d.categories.push(category);
              else d.categories[index] = category;
            });
            close();
          }}
        />
      )}
      {dialog?.type === "transaction" && (
        <TransactionForm
          budget={budget}
          initial={dialog.transaction}
          isNew={dialog.isNew ?? !dialog.transaction}
          repeatable={dialog.isNew ?? !dialog.transaction}
          initialRepeat={dialog.repeat}
          defaultAccount={currentAccount?.id}
          close={close}
          onCreateCategory={newCategory.create}
          save={async (transaction, repeat) => {
            let before: Budget | undefined;
            await model.transform((b) => {
              before = b;
              return saveTransaction(b, transaction, repeat);
            });
            close();
            if (before) learn(before, transaction);
            const message = scheduledMessage(transaction, repeat);
            if (message) toast({ message, action: { label: t("Undo"), onClick: model.undo } });
          }}
        />
      )}
      {newCategory.dialog}
      {dialog?.type === "bankSync" && (
        <Modal title={t("Sync with your bank")} close={close} wide>
          <BankSync
            budget={budget}
            balances={balances}
            first={dialog.account?.id}
            transform={model.transform}
            close={close}
            onAsk={(account, problem) => {
              close();
              ask({ text: syncHelpText(account.name, problem) });
            }}
            onCategorize={categorize}
          />
        </Modal>
      )}
      {dialog?.type === "bankSyncDetails" && (
        <Modal title={t("Bank sync · {account}", { account: dialog.account.name })} close={close}>
          <BankSyncDetails
            budget={budget}
            account={budget.accounts.find((a) => a.id === dialog.account.id) ?? dialog.account}
            balances={balances}
            edit={edit}
            onSync={() => setDialog({ type: "bankSync", account: dialog.account })}
            onSettings={() => {
              close();
              openSettings("import");
            }}
            onAsk={() => {
              close();
              ask({ text: syncHelpText(dialog.account.name) });
            }}
            close={close}
          />
        </Modal>
      )}
      {currentSummary && dialog?.type === "reconcile" && (
        <ReconcileForm
          account={dialog.account}
          cleared={balances.get(dialog.account.id)?.cleared ?? dialog.account.openingBalance}
          money={money}
          close={close}
          save={async (balance) => {
            const reconciled = await api.reconcile(budget, dialog.account.id, balance, today(), id());
            edit((d) => Object.assign(d, reconciled));
            close();
          }}
        />
      )}
      {dialog?.type === "payment" && (
        <div inert={!currentSummary || undefined} aria-busy={!currentSummary || undefined} style={{ display: "contents" }}>
        <CardPaymentForm
          budget={budget}
          card={dialog.account}
          balances={balances}
          available={available}
          close={close}
          pay={async (payment) => {
            await transformCalculated((b) => saveTransaction(b, payment));
            close();
            const from = budget.accounts.find((a) => a.id === payment.accountId)?.name ?? "";
            const paid = { amount: money(-payment.amount), card: dialog.account.name, account: from };
            toast({
              // Dated ahead, it's a scheduled payment.
              message: payment.date > today()
                ? t("Scheduled {amount} to {card} from {account} for {date}.", { ...paid, date: formatDate(payment.date) })
                : t("Paid {amount} to {card} from {account}.", paid),
              action: { label: t("Undo"), onClick: model.undo },
            });
          }}
        />
        </div>
      )}
      {dialog?.type === "loanPayment" && dialogLoan && (
        <div inert={!currentSummary || undefined} aria-busy={!currentSummary || undefined} style={{ display: "contents" }}>
        <LoanPaymentForm
          budget={budget}
          loan={dialog.account}
          terms={dialogLoan}
          balances={balances}
          close={close}
          record={async (p) => {
            await transformCalculated(async (b) => {
              const paid = await api.recordLoanPayment(b, p.terms, p.from, p.amount, p.date, p.id);
              // A category chosen here becomes the loan's payment category when it had none.
              const missing = !paid.categories.some((c) => c.id === dialogLoan.categoryId);
              return missing
                ? { ...paid, loans: paid.loans?.map((l) => (l.accountId === p.terms.accountId ? { ...l, categoryId: p.terms.categoryId } : l)) }
                : paid;
            });
            close();
            toast({
              message: p.date > today()
                ? t("Recorded a {amount} payment to {account} for {date}.", { amount: money(p.amount), account: dialog.account.name, date: formatDate(p.date) })
                : t("Recorded a {amount} payment to {account}.", { amount: money(p.amount), account: dialog.account.name }),
              action: { label: t("Undo"), onClick: model.undo },
            });
          }}
        />
        </div>
      )}
      {currentSummary && dialog?.type === "planner" && dialogLoan && (
        <LoanPlanner
          budget={budget}
          account={dialog.account}
          terms={dialogLoan}
          balance={balances.get(dialog.account.id)?.balance ?? dialog.account.openingBalance}
          close={close}
        />
      )}
      {dialog?.type === "confirm" && (
        <Modal title={dialog.title} close={close}>
          <p>{dialog.text}</p>
          <div className="modal-actions">
            <button className="secondary" onClick={close}>
              {t("Cancel")}
            </button>
            <button
              className="primary"
              onClick={async () => {
                try {
                  await dialog.action();
                  close();
                } catch (e) {
                  fail(e);
                }
              }}
            >
              {dialog.label}
            </button>
          </div>
        </Modal>
      )}
    </div>
    </LoanActionsContext.Provider>
    </HeaderSlotElements.Provider>
  );
}
