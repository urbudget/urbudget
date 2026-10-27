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
import { list, localDateTime, msg, t, tn, type LanguageSetting } from "./i18n";
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
        <strong>{t("Backups to keep")}</strong>
        <small>{t("Applies to each budget's automatic and manual backups. Older copies are removed when a new backup is made.")}</small>
      </span>
      <select aria-label={t("Backups to keep")} value={String(keep)} disabled={busy} onChange={async (e) => {
        const value = Number(e.target.value);
        setBusy(true);
        try {
          if (desktop) await api.setBackupRetention(value);
          setKeep(value);
        } catch (error) { fail(error); }
        finally { setBusy(false); }
      }}>
        <option value="0">{t("All of them")}</option>
        <option value="5">{tn(5, "The newest {count}", "The newest {count}")}</option>
        <option value="10">{tn(10, "The newest {count}", "The newest {count}")}</option>
        <option value="30">{tn(30, "The newest {count}", "The newest {count}")}</option>
      </select>
    </div>
  );
}

/** The app's language. Language names are always in their own language. */
function LanguageChoice() {
  const [value, setValue] = usePref("language");
  return (
    <div className="setting-row">
      <span>
        <strong>{t("Language")}</strong>
        <small>{t("System follows your computer's language. Your budget's names and formats stay as they are.")}</small>
      </span>
      <select aria-label={t("Language")} value={value} onChange={(e) => setValue(e.target.value as LanguageSetting)}>
        <option value="system">{t("System")}</option>
        <option value="en" lang="en">
          English
        </option>
        <option value="ar" lang="ar">
          العربية
        </option>
      </select>
    </div>
  );
}

// Key names stay as printed on keyboards; these also hold words (click, or), which are translated.
const KEYS_WITH_WORDS = new Set<string>([
  msg("Click, Ctrl+click, Shift+click"),
  msg("F2 or double-click"),
  msg("Enter, F2 or double-click"),
  msg("/ or Ctrl+F"),
]);

/** "Ctrl+Z / Ctrl+Shift+Z" as separate key caps in this system's notation, keeping the separators as text. */
const Keys = ({ keys }: { keys: string }) => (
  <>
    {keyLabel(KEYS_WITH_WORDS.has(keys) ? t(keys) : keys).split(/( \/ |, |، | or | أو )/).map((part, i) =>
      i % 2 ? (
        <span key={i} className={part === ", " || part === "، " ? "muted comma" : "muted"}>
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
    msg("Anywhere"),
    [
      ["Ctrl+Z / Ctrl+Shift+Z", msg("Undo / redo")],
      ["Ctrl+S", msg("Save now")],
      ["Ctrl+B", msg("Collapse or expand the sidebar")],
      ["Ctrl+J", msg("Open or close the assistant")],
    ],
  ],
  [
    msg("Budget"),
    [
      ["Click, Ctrl+click, Shift+click", msg("Select categories (the inspector shows them)")],
      ["F2 or double-click", msg("Rename a category or group")],
      ["Alt+↑ / Alt+↓", msg("Move a category up or down")],
      ["Enter", msg("In Assigned: save and move to the next category")],
      ["Esc", msg("Clear the selection")],
    ],
  ],
  [
    msg("Transactions"),
    [
      ["↑ / ↓, Shift+↑ / Shift+↓", msg("Move, or extend the selection")],
      ["Home / End, Page Up / Page Down", msg("Jump to the first or last row, or a page at a time")],
      ["Enter, F2 or double-click", msg("Edit the transaction in place")],
      ["Space", msg("Select or unselect the row")],
      ["Ctrl+A", msg("Select every listed transaction")],
      ["C", msg("Mark the selection cleared (again to unclear)")],
      ["A", msg("Approve the selection")],
      ["Ctrl+D", msg("Duplicate the selection")],
      ["Delete", msg("Delete the selection (with Undo)")],
      ["N", msg("Add a transaction")],
      ["/ or Ctrl+F", msg("Search")],
      ["Esc", msg("Clear the selection")],
    ],
  ],
  [
    msg("Editing a transaction"),
    [
      ["Enter", msg("Save")],
      ["Shift+Enter", msg("Save and add another")],
      ["Tab / Shift+Tab", msg("Next / previous field")],
      ["Esc", msg("Cancel")],
    ],
  ],
  [
    msg("Dates"),
    [
      // What the date fields read: these words stay as typed.
      ["15/10, 15 Oct, today", msg("Type a date in your format, or a word")],
      ["↑ / ↓", msg("A day later / earlier")],
      ["Page Up / Page Down", msg("A month later / earlier")],
      ["Alt+↓", msg("Open the calendar (arrows move, Enter picks)")],
    ],
  ],
  [
    msg("Search"),
    [
      // Search words are typed as they are, in any language.
      ["payee:, category:, account:, memo:", msg("Search one field")],
      ["amount:25, >100, <=50", msg("Search by amount")],
      ["is:cleared, is:red, is:unapproved, is:uncategorized", msg("Search by status, flag, approval or a missing category")],
      ["Enter / Backspace", msg("Add a filter chip / remove the last one")],
    ],
  ],
];

/** What a YNAB import brought in and what was checked against YNAB's own totals. */
function importSummary(budget: Budget) {
  const audit = budget.importAudit!;
  const fromFiles = budget.importedFrom === "ynabFiles";
  const source = budget.importCounts?.sourceTransactions ?? 0;
  const transfers = budget.importCounts?.transferPairs ?? 0;
  const checks = [
    [audit.accountChecks, (n: number) => tn(n, "{count} account balances", "{count} account balances")] as const,
    [audit.categoryMonthChecks, (n: number) => tn(n, "{count} category months", "{count} category months")] as const,
    [audit.monthChecks, (n: number) => tn(n, "{count} monthly unassigned totals", "{count} monthly unassigned totals")] as const,
  ]
    .filter(([n]) => n > 0)
    .map(([n, what]) => what(n));
  const differences =
    audit.accountDifferences.length + audit.categoryDifferences.length + audit.readyToAssignDifferences.length;
  const values = {
    accounts: tn(budget.accounts.length, "{count} accounts", "{count} accounts"),
    rows: fromFiles
      ? tn(source, "{count} register rows", "{count} register rows")
      : tn(source, "{count} transactions", "{count} transactions"),
    transfers: tn(transfers, "{count} linked transfers", "{count} linked transfers"),
    checks: list(checks),
    differences: tn(differences, "{count} differences", "{count} differences"),
  };
  return fromFiles
    ? t("Imported from YNAB's export files: {accounts}, {rows} ({transfers}). Checked {checks}; {differences}.", values)
    : t("Imported from YNAB: {accounts}, {rows} ({transfers}). Checked {checks}; {differences}.", values);
}

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
    path && toast({ message, action: { label: t("Show in folder"), onClick: reveal(path) } });
  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });

  return (
    <div className="page settings-page">
      <Tabs<Tab>
        label={t("Settings sections")}
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "general", label: t("General") },
          { id: "display", label: t("Budget & transactions") },
          { id: "import", label: t("Import") },
          { id: "assistant", label: t("Assistant") },
          { id: "backup", label: t("Backup & export") },
          { id: "sync", label: t("Account & sync") },
          { id: "shortcuts", label: t("Shortcuts") },
          { id: "advanced", label: t("Advanced") },
        ]}
      />

      {tab === "sync" && <SyncSettings cloud={cloud} budgetId={budgetId} isClean={isClean} openBudget={openBudget} />}

      {tab === "general" && (
        <>
          <PrivacySettings />
          <section className="panel">
            <h2>{t("Budget")}</h2>
            <label className="field-wide">
              {t("Budget name")}
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
            <p className="help">{t("The currency only changes how amounts are labelled. Amounts are never converted.")}</p>
          </section>
          <section className="panel">
            <h2>{t("App")}</h2>
            <p className="help">{t("These apply on this computer, to every budget.")}</p>
            <div className="setting-list">
              {desktop && <UpdateSetting updates={updates} version={version} onInstall={onInstallUpdate} />}
              {desktop && (
                <Toggle
                  pref="autoUpdate"
                  title={t("Install updates by themselves")}
                  help={t("Once one has downloaded, UrBudget installs it when it has sat unused for a while (a Microsoft Store copy, also when you close it), saving your budget first. Off, updates wait for you to install them.")}
                />
              )}
              <LanguageChoice />
              <div className="setting-row">
                <span>
                  <strong>{t("Theme")}</strong>
                  <small>{t("Light, dark, or whatever your system uses.")}</small>
                </span>
                <Tabs<Theme>
                  label={t("Theme")}
                  value={theme}
                  onChange={setTheme}
                  tabs={[
                    { id: "system", label: t("Match system") },
                    { id: "light", label: t("Light") },
                    { id: "dark", label: t("Dark") },
                  ]}
                />
              </div>
              <Choice
                pref="startPage"
                title={t("Open on")}
                help={t("The page you see when the app starts or you switch budgets.")}
                options={[
                  ["budget", t("Budget")],
                  ["transactions", t("All transactions")],
                ]}
              />
              <Choice
                pref="weekStart"
                title={t("Weeks start on")}
                help={t("The first column of the calendar in date fields.")}
                options={[
                  ["auto", t("As set on this computer")],
                  ["0", t("Sunday")],
                  ["1", t("Monday")],
                  ["6", t("Saturday")],
                ]}
              />
              <div className="setting-row">
                <span>
                  <strong>{t("Layout")}</strong>
                  <small>{t("Put column widths, collapsed groups and the sidebar back to how they started.")}</small>
                </span>
                <button
                  className="secondary"
                  onClick={() => {
                    onResetLayout();
                    toast({ message: t("Layout reset.") });
                  }}
                >
                  <RotateCcw size={14} /> {t("Reset layout")}
                </button>
              </div>
            </div>
          </section>
        </>
      )}

      {tab === "display" && (
        <>
          <section className="panel">
            <h2>{t("Budget")}</h2>
            <div className="setting-list">
              <Toggle
                pref="showNeeded"
                title={t("Show what targets still need")}
                help={t("The amount beside each category's name, e.g. “120.00 needed”.")}
              />
              <Toggle
                pref="showAgeOfMoney"
                title={t("Show cash age")}
                help={t("In the summary above the categories.")}
              />
            </div>
          </section>
          <section className="panel">
            <h2>{t("Transactions")}</h2>
            <div className="setting-list">
              <Toggle
                pref="newCleared"
                title={t("New transactions start cleared")}
                help={t("Useful if you enter transactions once they've reached your bank.")}
              />
              <Toggle
                pref="showUpcoming"
                title={t("Open scheduled transactions")}
                help={t("Schedules are listed at the top of each register, under a heading that folds them away. This sets whether they start open; the heading or Filters switch it there.")}
              />
              <Toggle
                pref="runningBalance"
                title={t("Show a running balance")}
                help={t("A balance column in each account's register, when sorted by date.")}
              />
              <Toggle
                pref="flagColumn"
                title={t("Show a Flag column")}
                help={t("Click a transaction's flag to set it. Name the flags from the same menu.")}
              />
            </div>
          </section>
        </>
      )}

      {tab === "shortcuts" && (
        <section className="panel">
          <h2>{t("Keyboard shortcuts")}</h2>
          <div className="shortcut-groups">
            {SHORTCUTS.map(([group, rows]) => (
              <div key={group} className="shortcut-group">
                <h3>{t(group)}</h3>
                <dl>
                  {rows.map(([keys, what]) => (
                    <div key={keys}>
                      <dt>
                        <Keys keys={keys} />
                      </dt>
                      <dd>{t(what)}</dd>
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
            <h2>{t("Import a bank file")}</h2>
            <p className="help">
              {t("Your bank's CSV or Excel file, or its OFX, QFX, QIF, CAMT.053 or MT940 statement; a PDF through the assistant. You see every line of the file and fix what doesn't read, then check what each transaction does, before anything is saved.")}
            </p>
            <div className="form-actions start">
              <button className="primary" onClick={onBankImport} disabled={!budget.accounts.some((a) => !a.archived)}>
                <FileUp size={15} /> {t("Import a bank file")}
              </button>
            </div>
          </section>
        </>
      )}

      {tab === "backup" && (
        <>
          <section className="panel">
            <h2>{t("Back up")}</h2>
            <p className="help">
              {/* "backups" is the folder's name on disk. */}
              {t("A backup is a copy of this budget's database file, kept in the {folder} folder next to it. Safety copies are made before replacing imported or synced data.", { folder: <em>backups</em> })}
            </p>
            <button
              className="primary"
              disabled={backingUp}
              onClick={async () => {
                setBackingUp(true);
                try {
                  if (!(await flush())) throw new Error(t("Save or resolve your changes first."));
                  const path = await api.backup();
                  saved(t("Backup created."), path);
                } catch (e) {
                  fail(e);
                } finally {
                  setBackingUp(false);
                }
              }}
            >
              <Database size={15} /> {t("Back up now")}
            </button>
          </section>
          <section className="panel">
            <h2>{t("Automatic backups")}</h2>
            <div className="setting-list">
              <Toggle
                pref="backupOnClose"
                title={t("Back up when the app closes")}
                help={t("A copy of this budget's database, saved in its backups folder each time you close the app.")}
              />
              <BackupRetention fail={fail} />
            </div>
          </section>
          <section className="panel">
            <h2>{t("Export and restore")}</h2>
            {budget.importAudit && (
              <div className="import-result">
                <p>{importSummary(budget)}</p>
                <YnabScreenAudit audit={budget.importAudit} />
              </div>
            )}
            <div className="setting-list">
              <div className="setting-row">
                <span>
                  <strong>{t("Full backup file")}</strong>
                  <small>{t("Everything in this budget, including unsaved changes, as one .json file.")}</small>
                </span>
                <button className="secondary" onClick={onExport}>
                  <Download size={15} /> {t("Export")}
                </button>
              </div>
              <div className="setting-row">
                <span>
                  <strong>{t("Restore from a backup file")}</strong>
                  <small>{t("Replaces this budget with a .json backup. A database backup is made first.")}</small>
                </span>
                <button className="secondary" onClick={() => importRef.current?.click()}>
                  <ArrowDownLeft size={15} /> {t("Restore…")}
                </button>
              </div>
              <div className="setting-row">
                <span>
                  <strong>{t("Export for YNAB")}</strong>
                  <small>{t("Plan and register CSV files in YNAB's format.")}</small>
                </span>
                <button
                  className="secondary"
                  onClick={async () => {
                    try {
                      saved(t("YNAB CSV files exported."), await api.exportYnab(budget));
                    } catch (e) {
                      fail(e);
                    }
                  }}
                >
                  <Download size={15} /> {t("Export CSV")}
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
              <h2>{t("Saving")}</h2>
              <span className={`status-badge ${sync}`}>{syncLabel}</span>
            </div>
            <p className="help">
              {t("Changes save automatically a moment after you stop editing. If the budget file changes elsewhere, it reloads when you have no unsaved changes; otherwise saving pauses so nothing is overwritten.")}
            </p>
            <dl className="detail-list">
              <div>
                <dt>{t("Budget file")}</dt>
                <dd className="mono">{metadata?.databasePath}</dd>
              </div>
              <div>
                <dt>{t("Last saved")}</dt>
                <dd>{metadata?.savedAt ? localDateTime(new Date(metadata.savedAt)) : t("Not yet")}</dd>
              </div>
              {version && (
                <div>
                  <dt>{t("App version")}</dt>
                  <dd>{version}</dd>
                </div>
              )}
            </dl>
            <div className="button-row">
              <button className="secondary" onClick={() => void flush()} disabled={sync === "conflict"}>
                <Check size={15} /> {t("Save now")}
              </button>
              <button className="secondary" onClick={onReload}>
                <RefreshCw size={15} /> {t("Reload from disk")}
              </button>
              {metadata?.databasePath && (
                <button className="secondary" onClick={reveal(metadata.databasePath)}>
                  <FolderOpen size={15} /> {t("Show in folder")}
                </button>
              )}
            </div>
          </section>
          <section className="panel muted">
            <h2>{t("How this budget works")}</h2>
            <p>
              {t("Cash accounts fund your plan. Tracking accounts count toward net worth but stay outside the plan. Card purchases from funded categories move money into the card's payment category; payments use that money.")}
            </p>
            <p>
              {t("Positive category balances roll over. Cash overspending reduces next month's Ready to Assign; credit overspending becomes card debt. Your budget is stored only on this computer and is not encrypted; your operating system protects access to the file.")}
            </p>
          </section>
        </>
      )}
    </div>
  );
}
