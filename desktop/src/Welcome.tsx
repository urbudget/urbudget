// What someone sees before they have a budget, what an empty budget shows, and the sample
// budget's banner. Both first screens lead to the setup walkthrough (SetupWalkthrough.tsx).
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, ChevronRight, FlaskConical } from "lucide-react";
import { api } from "./bridge";
import { BudgetFormatFields, type FormatValue } from "./BudgetFormatFields";
import { YnabFileImport } from "./YnabFileImport";
import { YnabImport } from "./YnabImport";
import { demoBudget } from "./demo";
import { dateFormats, errorText } from "./format";
import { localeDefaults } from "./locale";
import { newDraft, readDraft, readMark, writeDraft } from "./setup";
import { useToast } from "./ui";
import { AppMark } from "./WindowControls";
import { ltr, msg, t, useLanguage } from "./i18n";
import type { Budget, ImportResult, YnabImportRequest } from "./types";
import "./welcome.css";

/** The currency a budget made without one is in (Budget::default in domain.rs). */
const BUILT_IN_CURRENCY = "SAR";
/** The sample budget's name: made in the language in use, found by either name. */
const SAMPLE_NAME = msg("Sample budget");

/** No accounts, categories or transactions: a budget nobody has set up yet. */
export const isBlank = (b: Budget) => !b.accounts.length && !b.categories.length && !b.transactions.length;

/** An empty budget still in the built-in currency and formats nobody chose. */
export const isUntouched = (b: Budget) =>
  isBlank(b) && !b.assignments.length && b.currency === BUILT_IN_CURRENCY && !b.numberFormat && !b.dateFormat;

/** Opens the sample budget, making it (in this budget's currency and formats) the first time. */
export async function openSample(budget: Budget, budgetId: string, openBudget: (id: string) => Promise<void>) {
  const existing = (await api.listBudgets()).find((b) => (b.name === SAMPLE_NAME || b.name === t(SAMPLE_NAME)) && !b.missing && (!b.lifecycle || b.lifecycle === "active"));
  const id =
    existing?.id ??
    (await api.createBudget({
      ...demoBudget(),
      name: t(SAMPLE_NAME),
      currency: budget.currency,
      numberFormat: budget.numberFormat,
      dateFormat: budget.dateFormat,
      sample: { from: budgetId },
    }));
  await openBudget(id);
}

/** What the one button starts. The walkthrough teaches the method by doing it, so nothing explains it here. */
const STARTS = msg("Five short steps to a budget where all your money has a job. Skip any time.");

/** YNAB's token import into the open budget, with its export file a click away. */
function FromYnab({
  budget,
  openBudget,
  onBudget,
  importYnab,
  acceptYnab,
  resolveYnab,
  onAsk,
}: {
  budget: Budget;
  openBudget: (id: string) => Promise<void>;
  /** Takes a budget read from YNAB's export file. */
  onBudget: (imported: Budget) => Promise<void>;
  importYnab: (request: YnabImportRequest) => Promise<ImportResult>;
  acceptYnab: (reviewId: string) => Promise<ImportResult>;
  resolveYnab: (id: string, choices: Record<string, string>) => Promise<ImportResult>;
  onAsk?: (text: string) => void;
}) {
  const [exportFile, setExportFile] = useState(false);
  return (
    <div className="welcome-step">
      {!exportFile && (
        <p className="help">
          {t("Make a token in YNAB on the web (Account Settings, then Developer Settings, then New Token), paste it below, pick your plan and choose Import & save.")}
        </p>
      )}
      {exportFile ? (
        <YnabFileImport budget={budget} openBudget={openBudget} onBudget={onBudget} />
      ) : (
        <YnabImport onImport={importYnab} onAccept={acceptYnab} onResolve={resolveYnab} onAsk={onAsk} blank />
      )}
      <p className="help">
        <button className="link-button" onClick={() => setExportFile(!exportFile)}>
          {exportFile ? t("Use a YNAB token instead") : t("No token? Use YNAB's export file")}
        </button>
        {!exportFile && t(" (it leaves out targets, notes and scheduled transactions).")}
      </p>
    </div>
  );
}

/**
 * An empty budget's page: one button into the setup walkthrough, with the sample and YNAB as
 * quiet ways around it.
 */
export function Welcome({
  budget,
  budgetId,
  transform,
  openBudget,
  onSetup,
  importYnab,
  acceptYnab,
  resolveYnab,
  onAsk,
}: {
  budget: Budget;
  budgetId: string;
  transform: (work: (b: Budget) => Promise<Budget>) => Promise<void>;
  /** Switches to another budget (saving this one first). */
  openBudget: (id: string) => Promise<void>;
  /** Opens the setup walkthrough on this budget. */
  onSetup: () => void;
  /** YNAB's API import into this budget, as Settings › Import runs it. */
  importYnab: (request: YnabImportRequest) => Promise<ImportResult>;
  acceptYnab: (reviewId: string) => Promise<ImportResult>;
  resolveYnab: (id: string, choices: Record<string, string>) => Promise<ImportResult>;
  /** Opens the assistant with a question. */
  onAsk?: (text: string) => void;
}) {
  const toast = useToast();
  // A budget the first screen made to import YNAB into opens on the import.
  const [ynab, setYnab] = useState(() => !!budgetId && !!sessionStorage.getItem("lb:welcome-ynab:" + budgetId));
  useEffect(() => {
    if (budgetId) sessionStorage.removeItem("lb:welcome-ynab:" + budgetId);
  }, [budgetId]);
  const [busy, setBusy] = useState(false);
  const sample = () => {
    setBusy(true);
    openSample(budget, budgetId, openBudget)
      .catch((e) => toast({ message: errorText(e), tone: "error" }))
      .finally(() => setBusy(false));
  };
  return (
    <div className="page welcome">
      <div className="welcome-intro">
        <h2>{t("Let's set up your budget")}</h2>
        <p>{t(STARTS)}</p>
      </div>
      {!ynab && (
        <div className="welcome-start">
          <button className="primary welcome-go" disabled={busy} onClick={onSetup}>
            {readMark(budgetId) ? t("Continue setup") : t("Set up this budget")}
          </button>
        </div>
      )}
      <p className="welcome-links">
        <button className="link-button" disabled={busy} onClick={sample}>
          {t("Look around a sample")}
        </button>
        <button className="link-button" disabled={busy} aria-expanded={ynab} onClick={() => setYnab(!ynab)}>
          {ynab ? t("Set it up step by step instead") : t("I'm coming from YNAB")}
        </button>
      </p>
      {ynab && (
        <FromYnab
          budget={budget}
          openBudget={openBudget}
          onBudget={(imported) => transform(async () => imported)}
          importYnab={importYnab}
          acceptYnab={acceptYnab}
          resolveYnab={resolveYnab}
          onAsk={onAsk}
        />
      )}
    </div>
  );
}

/** Says the open budget is the sample, with ways back to a real one. */
export function SampleBanner({
  budget,
  budgetId,
  openBudget,
  onNewBudget,
}: {
  budget: Budget;
  budgetId: string;
  openBudget: (id: string) => Promise<void>;
  onNewBudget: () => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  if (!budget.sample) return null;
  const leave = async (remove: boolean) => {
    setBusy(true);
    try {
      const others = (await api.listBudgets()).filter((b) => b.id !== budgetId && !b.missing && (!b.lifecycle || b.lifecycle === "active"));
      const target = others.find((b) => b.id === budget.sample?.from) ?? others[0];
      if (!target) return onNewBudget();
      await openBudget(target.id);
      if (remove) {
        await api.deleteBudget(budgetId);
        toast({ message: t("Deleted the sample budget.") });
      }
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="notice sample-banner" role="note">
      <FlaskConical size={16} />
      <span>
        <strong>{t("This is a sample budget.")}</strong> {t("Try anything here; your own budget is separate.")}
      </span>
      <button className="primary" disabled={busy} onClick={() => void leave(false)}>
        {t("Go to my budget")}
      </button>
      <button className="secondary" disabled={busy} onClick={() => void leave(true)}>
        {t("Delete sample")}
      </button>
    </div>
  );
}

type Way = "ynab" | "file" | "backup" | "sync";

/**
 * A first launch: one button into the setup walkthrough, the device's currency and date format
 * with a way to change them, and two quiet ways around it. No budget is made until the
 * walkthrough is skipped or assigns the money, or one of the other ways brings a budget in.
 */
export function FirstRunWelcome({
  openBudget,
  onSetup,
  sync,
}: {
  openBudget: (id: string) => Promise<void>;
  /** Opens the setup walkthrough on the welcome screen's draft. */
  onSetup: () => void;
  /** Account and Sync, shown behind "I already have a budget". */
  sync?: ReactNode;
}) {
  const toast = useToast();
  const language = useLanguage();
  // Answers from an earlier sitting, offered again.
  const [draft] = useState(() => readDraft("welcome"));
  const [format, setFormat] = useState<FormatValue>(() => draft?.format ?? localeDefaults());
  const [changing, setChanging] = useState(false);
  const [existing, setExisting] = useState(false);
  const [way, setWay] = useState<Way | null>(null);
  const [busy, setBusy] = useState(false);
  const backup = useRef<HTMLInputElement>(null);
  const blank: Budget = { schemaVersion: 2, name: t("My budget"), ...format, accounts: [], categories: [], transactions: [], assignments: [] };
  const currency = useMemo(() => {
    try {
      return new Intl.DisplayNames([language], { type: "currency" }).of(format.currency) ?? format.currency;
    } catch {
      return format.currency;
    }
  }, [language, format.currency]);
  const run = (work: () => Promise<unknown>) => {
    setBusy(true);
    work()
      .catch((e) => toast({ message: errorText(e), tone: "error" }))
      .finally(() => setBusy(false));
  };
  const create = async (budget: Budget) => openBudget(await api.createBudget(budget));
  const start = () => {
    writeDraft(draft ? { ...draft, format } : newDraft("welcome", { name: t("My budget"), format }));
    onSetup();
  };
  // YNAB's token import saves into an open budget, so it gets one to land in first.
  const fromYnab = () =>
    run(async () => {
      const id = await api.createBudget(blank);
      sessionStorage.setItem("lb:welcome-ynab:" + id, "on");
      await openBudget(id);
    });
  const restore = (file: File) =>
    run(async () => {
      const raw = JSON.parse(await file.text());
      await create(raw.data?.plan || raw.data?.budget ? await api.importYnab(raw) : (raw as Budget));
    });

  if (existing) {
    const ways: { id: Way; title: string; text: string; run?: () => void }[] = [
      { id: "ynab", title: t("Bring it over from YNAB"), text: t("Paste a YNAB token and bring everything over in one click, checked against YNAB's own numbers."), run: fromYnab },
      { id: "file", title: t("Use YNAB's export file"), text: t("The file YNAB lets you download. It leaves out targets, notes and scheduled transactions.") },
      { id: "backup", title: t("Restore a backup file"), text: t("A .json backup that UrBudget made."), run: () => backup.current?.click() },
      { id: "sync", title: t("Sign in and sync"), text: t("Open the budgets in your UrBudget account.") },
    ];
    return (
      <div className="page welcome">
        <p>
          <button className="link-button" onClick={() => setExisting(false)}>
            <ArrowLeft size={14} className="welcome-back" /> {t("Back")}
          </button>
        </p>
        <div className="welcome-intro">
          <h2>{t("I already have a budget")}</h2>
          <p>{t("Bring it in from where it is now.")}</p>
        </div>
        <ul className="welcome-ways">
          {ways.map((w) => (
            <li key={w.id}>
              <button
                className={`welcome-way${way === w.id ? " selected" : ""}`}
                aria-expanded={w.run ? undefined : way === w.id}
                disabled={busy}
                onClick={() => (w.run ? w.run() : setWay(way === w.id ? null : w.id))}
              >
                <span>
                  <strong>{w.title}</strong>
                  <small>{w.text}</small>
                </span>
                <ChevronRight size={15} className="welcome-arrow" />
              </button>
              {way === w.id && w.id === "file" && (
                <div className="welcome-step">
                  <YnabFileImport budget={blank} openBudget={openBudget} onBudget={create} />
                </div>
              )}
              {way === w.id && w.id === "sync" && <div className="welcome-sync">{sync}</div>}
            </li>
          ))}
        </ul>
        <input
          ref={backup}
          type="file"
          accept=".json,application/json"
          hidden
          aria-label={t("Restore a backup file")}
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) restore(file);
            e.target.value = "";
          }}
        />
      </div>
    );
  }
  // The whole of it sits in the middle of the window, under the app's mark, as the phones' welcome does.
  return (
    <div className="page welcome welcome-first">
      <AppMark className="welcome-mark" />
      <div className="welcome-intro">
        <h2>{t("Let's set up your budget")}</h2>
        <p>{t(STARTS)}</p>
      </div>
      <div className="welcome-start">
        <button className="primary welcome-go" disabled={busy} onClick={start}>
          {draft ? t("Continue setup") : t("Create my budget")}
        </button>
        <p className="welcome-guess">
          <span>{currency}</span>
          <span aria-hidden> · </span>
          <span>{ltr(dateFormats[format.dateFormat])}</span>{" "}
          <button className="link-button" aria-expanded={changing} onClick={() => setChanging(!changing)}>
            {t("Change")}
          </button>
        </p>
        {changing && (
          <div className="welcome-format panel">
            <BudgetFormatFields value={format} onChange={setFormat} />
          </div>
        )}
      </div>
      <p className="welcome-links">
        <button className="link-button" disabled={busy} onClick={() => run(() => openSample(blank, "", openBudget))}>
          {t("Look around a sample")}
        </button>
        <button className="link-button" disabled={busy} onClick={() => setExisting(true)}>
          {t("I already have a budget")}
        </button>
      </p>
    </div>
  );
}
