import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { ArrowDownLeft, Check, Database, Download, FileUp, FolderOpen, RefreshCw, RotateCcw } from "lucide-react";
import { api, desktop } from "./bridge";
import { BankSyncSettings } from "./BankSync";
import { YnabImport } from "./YnabImport";
import { YnabFileImport } from "./YnabFileImport";
import { YnabScreenAudit } from "./YnabScreenAudit";
import { CommitInput } from "./forms";
import { BudgetFormatFields } from "./BudgetFormatFields";
import { Tabs, useToast } from "./ui";
import { errorText, numberFormatFor } from "./format";
import type { Theme } from "./theme";
import type { SyncState } from "./useBudget";
import type { Budget, DatabaseStatus, ImportResult, YnabImportRequest } from "./types";
import { usePref, type Prefs } from "./prefs";
import { keyLabel } from "./platform";
import { AssistantSettings } from "./assistant/AssistantSettings";
import { SyncSettings } from "./CloudSettings";
import type { Cloud } from "./cloud";
import type { Updates } from "./updates";
import { UpdateSetting } from "./UpdateNotice";
import { PrivacySettings } from "./PrivacySettings";
import "./settings.css";

export type Tab = "general" | "display" | "import" | "assistant" | "backup" | "sync" | "shortcuts" | "advanced";

/** One on/off preference, shown as a switch with a description. */
function Toggle<K extends keyof Prefs>({ pref, title, help }: { pref: K; title: string; help: string }) {
  const [value, setValue] = usePref(pref);
  return (
    <label className="setting-row toggle">
      <span>
        <strong>{title}</strong>
        <small>{help}</small>
      </span>
      <input type="checkbox" role="switch" checked={!!value} onChange={(e) => setValue(e.target.checked as Prefs[K])} />
    </label>
  );
}

/** One preference with a few choices. */
function Choice<K extends keyof Prefs>({
  pref,
  title,
  help,
  options,
}: {
  pref: K;
  title: string;
  help: string;
  options: [string, string][];
}) {
  const [value, setValue] = usePref(pref);
  return (
    <div className="setting-row">
      <span>
        <strong>{title}</strong>
        <small>{help}</small>
      </span>
      <select
        aria-label={title}
        value={String(value)}
        onChange={(e) => {
          const raw = e.target.value;
          setValue((typeof value === "number" ? Number(raw) : raw) as Prefs[K]);
        }}
      >
        {options.map(([v, label]) => (
          <option key={v} value={v}>
            {label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** Retention is persisted natively so automatic sync/import backups follow it without a window. */
function BackupRetention({ fail }: { fail: (e: unknown) => void }) {
  const [keep, setKeep] = usePref("keepBackups");
  const [busy, setBusy] = useState(desktop);
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    void api.backupRetention().then((value) => { if (active) setKeep(value); })
      .catch(fail).finally(() => { if (active) setBusy(false); });
    return () => { active = false; };
  }, []);
  return (
    <div className="setting-row">
      <span>
        <strong>Backups to keep</strong>
        <small>Applies to each budget's automatic and manual backups. Older copies are removed when a new backup is made.</small>
      </span>
      <select aria-label="Backups to keep" value={String(keep)} disabled={busy} onChange={async (e) => {
        const value = Number(e.target.value);
        setBusy(true);
        try {
          if (desktop) await api.setBackupRetention(value);
          setKeep(value);
        } catch (error) { fail(error); }
        finally { setBusy(false); }
      }}>
        <option value="0">All of them</option>
        <option value="5">The newest 5</option>
        <option value="10">The newest 10</option>
        <option value="30">The newest 30</option>
      </select>
    </div>
  );
}

/** "Ctrl+Z / Ctrl+Shift+Z" as separate key caps in this system's notation, keeping the separators as text. */
const Keys = ({ keys }: { keys: string }) => (
  <>
    {keyLabel(keys).split(/( \/ |, | or )/).map((part, i) =>
      i % 2 ? (
        <span key={i} className={part === ", " ? "muted comma" : "muted"}>
          {part.trim()}
        </span>
      ) : (
        <kbd key={i}>{part}</kbd>
      ),
    )}
  </>
);

const SHORTCUTS: [string, [string, string][]][] = [
  [
    "Anywhere",
    [
      ["Ctrl+Z / Ctrl+Shift+Z", "Undo / redo"],
      ["Ctrl+S", "Save now"],
      ["Ctrl+B", "Collapse or expand the sidebar"],
      ["Ctrl+J", "Open or close the assistant"],
    ],
  ],
  [
    "Budget",
    [
      ["Click, Ctrl+click, Shift+click", "Select categories (the inspector shows them)"],
      ["F2 or double-click", "Rename a category or group"],
      ["Alt+↑ / Alt+↓", "Move a category up or down"],
      ["Enter", "In Assigned: save and move to the next category"],
      ["Esc", "Clear the selection"],
    ],
  ],
  [
    "Transactions",
    [
      ["↑ / ↓, Shift+↑ / Shift+↓", "Move, or extend the selection"],
      ["Home / End, Page Up / Page Down", "Jump to the first or last row, or a page at a time"],
      ["Enter, F2 or double-click", "Edit the transaction in place"],
      ["Space", "Select or unselect the row"],
      ["Ctrl+A", "Select every listed transaction"],
      ["C", "Mark the selection cleared (again to unclear)"],
      ["A", "Approve the selection"],
      ["Ctrl+D", "Duplicate the selection"],
      ["Delete", "Delete the selection (with Undo)"],
      ["N", "Add a transaction"],
      ["/ or Ctrl+F", "Search"],
      ["Esc", "Clear the selection"],
    ],
  ],
  [
    "Editing a transaction",
    [
      ["Enter", "Save"],
      ["Shift+Enter", "Save and add another"],
      ["Tab / Shift+Tab", "Next / previous field"],
      ["Esc", "Cancel"],
    ],
  ],
  [
    "Dates",
    [
      ["15/10, 15 Oct, today", "Type a date in your format, or a word"],
      ["↑ / ↓", "A day later / earlier"],
      ["Page Up / Page Down", "A month later / earlier"],
      ["Alt+↓", "Open the calendar (arrows move, Enter picks)"],
    ],
  ],
  [
    "Search",
    [
      ["payee:, category:, account:, memo:", "Search one field"],
      ["amount:25, >100, <=50", "Search by amount"],
      ["is:cleared, is:red, is:unapproved, is:uncategorized", "Search by status, flag, approval or a missing category"],
      ["Enter / Backspace", "Add a filter chip / remove the last one"],
    ],
  ],
];

export function SettingsPage({
  budget,
  sync,
  syncLabel,
  metadata,
  edit,
  flush,
  onReload,
  onExport,
  onImportFile,
  importYnab,
  acceptYnab,
  resolveYnab,
  onAsk,
  openBudget,
  onBankSync,
  onBankImport,
  theme,
  setTheme,
  onResetLayout,
  initialTab,
  cloud,
  updates,
  onInstallUpdate,
  budgetId,
  isClean,
}: {
  cloud: Cloud;
  updates: Updates;
  /** Saves, then installs the waiting update and restarts. */
  onInstallUpdate: () => void;
  budgetId: string;
  /** The open budget has no unsaved edits. */
  isClean: () => boolean;
  /** The section to open on, such as "assistant" from the assistant's panel. */
  initialTab?: Tab;
  /** Puts column widths, collapsed groups and the sidebar back to their defaults. */
  onResetLayout: () => void;
  budget: Budget;
  sync: SyncState;
  syncLabel: string;
  metadata: DatabaseStatus | null;
  edit: (update: (draft: Budget) => void) => void;
  flush: () => Promise<boolean>;
  onReload: () => void;
  onExport: () => void;
  onImportFile: (file: File) => void;
  importYnab: (request: YnabImportRequest) => Promise<ImportResult>;
  acceptYnab: (reviewId: string) => Promise<ImportResult>;
  resolveYnab: (reviewId: string, choices: Record<string, string>) => Promise<ImportResult>;
  /** Opens the assistant with a question. */
  onAsk?: (text: string) => void;
  /** Switches to another budget (saving this one first), such as one just imported. */
  openBudget: (id: string) => Promise<void>;
  /** Syncs every account linked to a bank. */
  onBankSync: () => void;
  /** Opens the page that imports a bank file. */
  onBankImport: () => void;
  theme: Theme;
  setTheme: (theme: Theme) => void;
}) {
  const toast = useToast();
  const [tab, setTab] = useState<Tab>(initialTab ?? "general");
  const importRef = useRef<HTMLInputElement>(null);
  const [version, setVersion] = useState("");
  const [backingUp, setBackingUp] = useState(false);
  useEffect(() => {
    if (desktop) void getVersion().then(setVersion).catch(() => {});
  }, []);
  const reveal = (path: string) => () =>
    void api.reveal(path).catch((e) => toast({ message: errorText(e), tone: "error" }));
  // A null path is a cancelled save panel.
  const saved = (message: string, path: string | null) =>
    path && toast({ message, action: { label: "Show in folder", onClick: reveal(path) } });
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });

  return (
    <div className="page settings-page">
      <Tabs<Tab>
        label="Settings sections"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "general", label: "General" },
          { id: "display", label: "Budget & transactions" },
          { id: "import", label: "Import" },
          { id: "assistant", label: "Assistant" },
          { id: "backup", label: "Backup & export" },
          { id: "sync", label: "Account & sync" },
          { id: "shortcuts", label: "Shortcuts" },
          { id: "advanced", label: "Advanced" },
        ]}
      />

      {tab === "sync" && <SyncSettings cloud={cloud} budgetId={budgetId} isClean={isClean} openBudget={openBudget} />}

      {tab === "general" && (
        <>
          <PrivacySettings />
          <section className="panel">
            <h2>Budget</h2>
            <label className="field-wide">
              Budget name
              <CommitInput
                value={budget.name}
                maxLength={200}
                validate={(v) => !!v.trim()}
                onCommit={(name) =>
                  edit((d) => {
                    d.name = name.trim();
                  })
                }
              />
            </label>
            <BudgetFormatFields
              value={{
                currency: budget.currency,
                numberFormat: numberFormatFor(budget.currency, budget.numberFormat),
                dateFormat: budget.dateFormat ?? "iso",
              }}
              onChange={(v) =>
                edit((d) => {
                  d.currency = v.currency;
                  d.numberFormat = v.numberFormat;
                  d.dateFormat = v.dateFormat;
                })
              }
            />
            <p className="help">
              The currency only changes how amounts are labelled. Amounts are never converted.
            </p>
          </section>
          <section className="panel">
            <h2>App</h2>
            <p className="help">These apply on this computer, to every budget.</p>
            <div className="setting-list">
              {desktop && <UpdateSetting updates={updates} version={version} onInstall={onInstallUpdate} />}
              <div className="setting-row">
                <span>
                  <strong>Theme</strong>
                  <small>Light, dark, or whatever your system uses.</small>
                </span>
                <Tabs<Theme>
                  label="Theme"
                  value={theme}
                  onChange={setTheme}
                  tabs={[
                    { id: "system", label: "Match system" },
                    { id: "light", label: "Light" },
                    { id: "dark", label: "Dark" },
                  ]}
                />
              </div>
              <Choice
                pref="startPage"
                title="Open on"
                help="The page you see when the app starts or you switch budgets."
                options={[
                  ["budget", "Budget"],
                  ["transactions", "All transactions"],
                ]}
              />
              <Choice
                pref="weekStart"
                title="Weeks start on"
                help="The first column of the calendar in date fields."
                options={[
                  ["auto", `As set on this computer`],
                  ["0", "Sunday"],
                  ["1", "Monday"],
                  ["6", "Saturday"],
                ]}
              />
              <div className="setting-row">
                <span>
                  <strong>Layout</strong>
                  <small>Put column widths, collapsed groups and the sidebar back to how they started.</small>
                </span>
                <button
                  className="secondary"
                  onClick={() => {
                    onResetLayout();
                    toast({ message: "Layout reset." });
                  }}
                >
                  <RotateCcw size={14} /> Reset layout
                </button>
              </div>
            </div>
          </section>
        </>
      )}

      {tab === "display" && (
        <>
          <section className="panel">
            <h2>Budget</h2>
            <div className="setting-list">
              <Toggle
                pref="showNeeded"
                title="Show what targets still need"
                help="The amount beside each category's name, e.g. “120.00 needed”."
              />
              <Toggle
                pref="showAgeOfMoney"
                title="Show cash age"
                help="In the summary above the categories."
              />
            </div>
          </section>
          <section className="panel">
            <h2>Transactions</h2>
            <div className="setting-list">
              <Toggle
                pref="newCleared"
                title="New transactions start cleared"
                help="Useful if you enter transactions once they've reached your bank."
              />
              <Toggle
                pref="showUpcoming"
                title="Open scheduled transactions"
                help="Schedules are listed at the top of each register, under a heading that folds them away. This sets whether they start open; the heading or Filters switch it there."
              />
              <Toggle
                pref="runningBalance"
                title="Show a running balance"
                help="A balance column in each account's register, when sorted by date."
              />
              <Toggle
                pref="flagColumn"
                title="Show a Flag column"
                help="Click a transaction's flag to set it. Name the flags from the same menu."
              />
            </div>
          </section>
        </>
      )}

      {tab === "shortcuts" && (
        <section className="panel">
          <h2>Keyboard shortcuts</h2>
          <div className="shortcut-groups">
            {SHORTCUTS.map(([group, rows]) => (
              <div key={group} className="shortcut-group">
                <h3>{group}</h3>
                <dl>
                  {rows.map(([keys, what]) => (
                    <div key={keys}>
                      <dt>
                        <Keys keys={keys} />
                      </dt>
                      <dd>{what}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            ))}
          </div>
        </section>
      )}

      {tab === "assistant" && <AssistantSettings />}

      {tab === "import" && (
        <>
          <BankSyncSettings budget={budget} edit={edit} onSync={onBankSync} />
          <YnabFileImport budget={budget} openBudget={openBudget} />
          <YnabImport
            onResolve={resolveYnab}
            onAccept={acceptYnab}
            onAsk={onAsk}
            planId={budget.ynabPlanId}
            onImport={importYnab}
          />
          <section className="panel">
            <h2>Import a bank file</h2>
            <p className="help">
              Your bank's CSV or Excel file, or its OFX, QFX, QIF, CAMT.053 or MT940 statement; a PDF through the assistant.
              You see every line of the file and fix what doesn't read, then check what each transaction does, before
              anything is saved.
            </p>
            <div className="form-actions start">
              <button className="primary" onClick={onBankImport} disabled={!budget.accounts.some((a) => !a.archived)}>
                <FileUp size={15} /> Import a bank file
              </button>
            </div>
          </section>
        </>
      )}

      {tab === "backup" && (
        <>
          <section className="panel">
            <h2>Back up</h2>
            <p className="help">
              A backup is a copy of this budget's database file, kept in the <em>backups</em> folder next to it. Safety copies are made before replacing imported or synced data.
            </p>
            <button
              className="primary"
              disabled={backingUp}
              onClick={async () => {
                setBackingUp(true);
                try {
                  if (!(await flush())) throw new Error("Save or resolve your changes first.");
                  const path = await api.backup();
                  saved("Backup created.", path);
                } catch (e) {
                  fail(e);
                } finally {
                  setBackingUp(false);
                }
              }}
            >
              <Database size={15} /> Back up now
            </button>
          </section>
          <section className="panel">
            <h2>Automatic backups</h2>
            <div className="setting-list">
              <Toggle
                pref="backupOnClose"
                title="Back up when the app closes"
                help="A copy of this budget's database, saved in its backups folder each time you close the app."
              />
              <BackupRetention fail={fail} />
            </div>
          </section>
          <section className="panel">
            <h2>Export and restore</h2>
            {budget.importAudit && (
              <div className="import-result">
                <p>
                  Imported from YNAB{budget.importedFrom === "ynabFiles" ? "'s export files" : ""}:{" "}
                  {budget.accounts.length} accounts, {budget.importCounts?.sourceTransactions}{" "}
                  {budget.importedFrom === "ynabFiles" ? "register rows" : "transactions"} (
                  {budget.importCounts?.transferPairs} linked transfers). Checked{" "}
                  {[
                    [budget.importAudit.accountChecks, "account balances"] as const,
                    [budget.importAudit.categoryMonthChecks, "category months"] as const,
                    [budget.importAudit.monthChecks, "monthly unassigned totals"] as const,
                  ]
                    .filter(([n]) => n > 0)
                    .map(([n, what]) => `${n} ${what}`)
                    .join(", ")}
                  ;{" "}
                  {budget.importAudit.accountDifferences.length +
                    budget.importAudit.categoryDifferences.length +
                    budget.importAudit.readyToAssignDifferences.length}{" "}
                  differences.
                </p>
                <YnabScreenAudit audit={budget.importAudit} />
              </div>
            )}
            <div className="setting-list">
              <div className="setting-row">
                <span>
                  <strong>Full backup file</strong>
                  <small>Everything in this budget, including unsaved changes, as one .json file.</small>
                </span>
                <button className="secondary" onClick={onExport}>
                  <Download size={15} /> Export
                </button>
              </div>
              <div className="setting-row">
                <span>
                  <strong>Restore from a backup file</strong>
                  <small>Replaces this budget with a .json backup. A database backup is made first.</small>
                </span>
                <button className="secondary" onClick={() => importRef.current?.click()}>
                  <ArrowDownLeft size={15} /> Restore…
                </button>
              </div>
              <div className="setting-row">
                <span>
                  <strong>Export for YNAB</strong>
                  <small>Plan and register CSV files in YNAB's format.</small>
                </span>
                <button
                  className="secondary"
                  onClick={async () => {
                    try {
                      saved("YNAB CSV files exported.", await api.exportYnab(budget));
                    } catch (e) {
                      fail(e);
                    }
                  }}
                >
                  <Download size={15} /> Export CSV
                </button>
              </div>
            </div>
            <input
              ref={importRef}
              type="file"
              accept=".json,application/json"
              hidden
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (file) onImportFile(file);
                e.target.value = "";
              }}
            />
          </section>
        </>
      )}

      {tab === "advanced" && (
        <>
          <section className="panel">
            <div className="panel-title">
              <h2>Saving</h2>
              <span className={`status-badge ${sync}`}>{syncLabel}</span>
            </div>
            <p className="help">
              Changes save automatically a moment after you stop editing. If the budget file
              changes elsewhere, it reloads when you have no unsaved changes; otherwise saving
              pauses so nothing is overwritten.
            </p>
            <dl className="detail-list">
              <div>
                <dt>Budget file</dt>
                <dd className="mono">{metadata?.databasePath}</dd>
              </div>
              <div>
                <dt>Last saved</dt>
                <dd>{metadata?.savedAt ? new Date(metadata.savedAt).toLocaleString() : "Not yet"}</dd>
              </div>
              {version && (
                <div>
                  <dt>App version</dt>
                  <dd>{version}</dd>
                </div>
              )}
            </dl>
            <div className="button-row">
              <button className="secondary" onClick={() => void flush()} disabled={sync === "conflict"}>
                <Check size={15} /> Save now
              </button>
              <button className="secondary" onClick={onReload}>
                <RefreshCw size={15} /> Reload from disk
              </button>
              {metadata?.databasePath && (
                <button className="secondary" onClick={reveal(metadata.databasePath)}>
                  <FolderOpen size={15} /> Show in folder
                </button>
              )}
            </div>
          </section>
          <section className="panel muted">
            <h2>How this budget works</h2>
            <p>
              Cash accounts fund your plan. Tracking accounts count toward net worth but stay
              outside the plan. Card purchases from funded categories move money into the card's
              payment category; payments use that money.
            </p>
            <p>
              Positive category balances roll over. Cash overspending reduces next month's Ready
              to Assign; credit overspending becomes card debt. Your budget is stored only on this
              computer and is not encrypted; your operating system protects access to the file.
            </p>
          </section>
        </>
      )}
    </div>
  );
}
