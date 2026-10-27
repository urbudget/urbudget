import { useEffect, useMemo, useRef, useState, type ChangeEvent, type DragEvent } from "react";
import { Download, FileText, FileUp, LoaderCircle, MessageSquareText, Table2, X } from "lucide-react";
import { api, type BankFixes, type BankRow, type BankSession, type Statement } from "../bridge";
import {
  ImportReview,
  bankTemplate,
  statementFile,
  uncategorizedImports,
  type ImportChoice,
  type ReviewChoices,
  type Uncategorized,
} from "../BankImport";
import { errorText } from "../format";
import type { Budget } from "../types";
import { Tabs, useToast } from "../ui";
import { useSteady } from "../useSteady";
import { StatementGrid } from "./StatementGrid";
import { applyToLatest, leftOut, needsEditor } from "./model";
import "./import-editor.css";

/** An import in progress, kept while someone looks elsewhere (the sidebar leads back to it). */
export type ImportState = {
  account: string;
  file?: File;
  /** The file's text (a workbook's sheet, as CSV), its sheets and the one read. */
  content?: string;
  sheets?: string[];
  sheet?: string | null;
  session?: BankSession;
  /** Where it is: every line, to fix them, or the review of those that read. */
  view?: "editor" | "review";
  /** The assistant prepared the file: its proposal, marked applied once imported. */
  proposal?: { threadId: string; id: string };
  /** Lines the assistant changed. */
  assisted?: string[];
  /** What's chosen in the review, for the session it was chosen in: it outlasts the review, shown again after the file. */
  choices?: ReviewChoices & { session: string };
};
const NO_CHOICES: ReviewChoices = { picked: {} };

/**
 * Importing a bank file, as a page: choose the file, see every line with what became of it and fix
 * what doesn't read (the editor opens by itself when something needs it), then review what each
 * line that reads does. Nothing is saved until the import.
 */
export function ImportPage({
  budget,
  budgetId,
  state,
  onState,
  transform,
  onImported,
  onCancel,
  onAskAssistant,
  onFixWithAssistant,
}: {
  budget: Budget;
  budgetId: string;
  state: ImportState;
  onState: (next: ImportState | ((was: ImportState) => ImportState)) => void;
  transform: (work: (b: Budget) => Promise<Budget>) => Promise<void>;
  /** After the import: how many lines were added or matched, and those left without a category. */
  onImported: (count: number, uncategorized: Uncategorized) => void;
  onCancel: () => void;
  /** Hands the file to the assistant, with what went wrong if anything did. */
  onAskAssistant?: (file: File, problem: string, account: string) => void;
  /** Asks the assistant to fix the lines that need attention (it's told them with the message). */
  onFixWithAssistant?: () => void;
}) {
  const toast = useToast();
  const [statement, setStatement] = useState<Statement | null>(null);
  const [review, setReview] = useState<{
    rows: BankRow[];
    base: Budget;
    statement: Statement;
  } | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  // The import being saved: it lands whatever is clicked meanwhile, so nothing leaves it.
  const [saving, setSaving] = useState(false);
  const [dragging, setDragging] = useState(false);
  const { account, file, content, session } = state;
  const accountName = budget.accounts.find((a) => a.id === account)?.name ?? "";
  const pdf = !!file && /\.pdf$/i.test(file.name);
  // The session a reply is for: one for another session (another file) is dropped.
  const current = useRef(session?.scope.session);
  current.current = session?.scope.session;
  // A new session shows nothing of the last one's.
  useEffect(() => {
    setStatement(null);
    setReview(null);
  }, [session?.scope.session]);

  // The file being read, so it's read once.
  const started = useRef<File | null>(null);
  const start = async (next: File, sheet: string | null = null) => {
    started.current = next;
    // The assistant's file, read again (another sheet), is still the assistant's.
    const proposal = state.file === next ? state.proposal : undefined;
    onState({ account, file: next, proposal });
    setStatement(null);
    setReview(null);
    setMessage("");
    if (/\.pdf$/i.test(next.name)) return;
    setBusy(true);
    try {
      // A workbook opens at the sheet the account's last one was read from, when it has one by that name.
      const read = await statementFile(next, sheet ?? budget.accounts.find((a) => a.id === account)?.fileImport?.sheet ?? null);
      // A file the assistant prepared is written the standard way, whatever the account's last one was like.
      const fresh = await api.bankSession(budget, account, read.text, crypto.randomUUID(), budgetId, read.sheet ?? null, !!proposal);
      onState((was) =>
        was.file === next
          ? {
              ...was,
              content: read.text,
              sheets: read.sheets,
              sheet: read.sheet ?? null,
              session: fresh,
              view: undefined,
            }
          : was,
      );
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  // A file handed over (the assistant's): read it at once.
  useEffect(() => {
    if (file && !session && !pdf && started.current !== file) void start(file, state.sheet ?? null);
  }, [file]);

  // The statement, read again as soon as a change lands (a reading for the change before it is
  // dropped). The review reads the file itself, so there it isn't read twice.
  const view = useRef(state.view);
  view.current = state.view;
  useEffect(() => {
    if (!session || content === undefined || view.current === "review") return;
    const stop = new AbortController();
    api.bankStatement(budget, account, content, session, stop.signal).then(
      (read) => {
        if (stop.signal.aborted || current.current !== read.scope.session) return;
        setStatement(read);
        if (!view.current)
          onState((was) => ({
            ...was,
            view: needsEditor(read) ? "editor" : "review",
          }));
      },
      (e) => !stop.signal.aborted && setMessage(errorText(e)),
    );
    return () => stop.abort();
  }, [session, content, account]);

  // The review, and again whenever the budget or the session changes.
  useEffect(() => {
    if (state.view !== "review" || !session || content === undefined) return;
    let live = true;
    api.bankStatementPreview(budget, account, content, session).then(
      (r) => {
        if (!live || current.current !== r.statement.scope.session) return;
        setReview({ rows: r.rows, base: budget, statement: r.statement });
        setStatement(r.statement);
      },
      (e) => live && setMessage(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [state.view, session, content, budget]);

  // The import as it is now, for a change that lands after another.
  const sessionNow = useRef(session);
  sessionNow.current = session;
  const fix = async (fixes: BankFixes) => {
    if (!session || content === undefined) return;
    setBusy(true);
    setMessage("");
    try {
      const applied = await applyToLatest(
        () => sessionNow.current,
        (base) => api.bankApplyFixes(budget, account, content, base, fixes),
      );
      // Another file since: its fix is for an import that's gone.
      if (!applied || current.current !== fixes.scope.session) return;
      onState((was) => (was.session === applied.base ? { ...was, session: applied.session } : was));
      const refused = applied.results.filter((r) => !r.applied);
      if (refused.length) setMessage(refused[0].problem ?? "That change no longer fits the file.");
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  // The review of the import as it is now: after a change in the editor, the last one is out of date.
  const reviewed = review && session && review.statement.revision === session.revision ? review : null;
  const save = async (choices: ImportChoice[], importing: number) => {
    if (!reviewed) return;
    setBusy(true);
    setSaving(true);
    setMessage("");
    try {
      let uncategorized: Uncategorized = { ids: [], accounts: [] };
      await transform(async (b) => {
        if (b !== reviewed.base) throw Error("Your budget changed. The review is up to date again: check it and import.");
        // The account keeps how its own file read, not the assistant's.
        const after = await api.importBank(b, account, choices, state.proposal ? null : reviewed.statement.remember);
        uncategorized = uncategorizedImports(b, after);
        return after;
      });
      onImported(importing, uncategorized);
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setBusy(false);
      setSaving(false);
    }
  };
  const template = async () => {
    try {
      const path = await api.writeExport(
        "bank-import-template.csv",
        bankTemplate(budget.accounts.find((a) => a.id === account)?.fileImport?.dateFormat ?? "ymd"),
      );
      if (!path) return;
      toast({
        message: "Template saved.",
        action: {
          label: "Show in folder",
          onClick: () => void api.reveal(path).catch((e) => setMessage(errorText(e))),
        },
      });
    } catch (e) {
      setMessage(errorText(e));
    }
  };
  const drop = (e: DragEvent<HTMLLabelElement>) => {
    e.preventDefault();
    setDragging(false);
    const dropped = e.dataTransfer.files?.[0];
    if (dropped) void start(dropped);
  };
  const shown = reviewed?.statement ?? statement;
  // On to the review from the file: the file stays in view until the review is ready.
  const [continuing, setContinuing] = useState(false);
  useEffect(() => {
    if (reviewed || state.view !== "review") setContinuing(false);
    // The review couldn't be made: back to the file, with why.
    else if (continuing && message) {
      setContinuing(false);
      onState((was) => ({ ...was, view: "editor" }));
    }
  }, [reviewed, state.view, message]);
  // A wait shows only once it's long enough to notice.
  const reading = useSteady(!!session && !statement && !message, (r) => r, 300);
  const checking = useSteady(state.view === "review" && !reviewed && !continuing && !message, (c) => c, 300);
  const assisted = useMemo(() => new Set(state.assisted ?? []), [state.assisted]);
  const out = shown ? leftOut(shown) : 0;
  const attention = shown?.counts.attention ?? 0;
  const problem = message || (shown && !shown.counts.ready ? "None of the file's lines could be read." : "");
  return (
    <section className="bank-import import-page panel">
      <header className="import-page-head">
        {/* The page is titled already: this says what goes where. */}
        <h2>
          {file ? (
            <>
              <span className="import-file-name">{file.name}</span>
              {accountName && <span className="import-into"> into {accountName}</span>}
            </>
          ) : (
            "Choose the file and its account"
          )}
        </h2>
        <span className="import-page-actions">
          {file && (
            <button
              className="link-button"
              disabled={saving || (busy && !session)}
              onClick={() => {
                onState({ account });
                setMessage("");
              }}
            >
              Choose another file
            </button>
          )}
          <button className="secondary small" disabled={saving} onClick={onCancel}>
            {session ? "Cancel the import" : "Close"}
          </button>
        </span>
      </header>
      {session && (state.sheets?.length ?? 0) > 1 && (
        <Tabs
          label="Sheets"
          className="statement-sheets"
          tabs={state.sheets!.map((name) => ({ id: name, label: name }))}
          value={state.sheet ?? state.sheets![0]}
          onChange={(sheet) => file && void start(file, sheet)}
        />
      )}
      {!session ? (
        <>
          <p className="import-lead">
            Bring in the transactions your bank exported. You see every line of the file and fix anything that doesn't read, then check what
            each transaction does before anything is saved.
          </p>
          <label className="import-account">
            Into account
            <select value={account} disabled={busy} onChange={(e) => onState({ account: e.target.value })}>
              {budget.accounts
                .filter((a) => !a.archived)
                .map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
            </select>
          </label>
          <label
            className={`file-drop${dragging ? " dragging" : ""}${file ? " chosen" : ""}`}
            onDragOver={(e) => {
              e.preventDefault();
              setDragging(true);
            }}
            onDragLeave={() => setDragging(false)}
            onDrop={drop}
          >
            {file ? <FileText size={22} aria-hidden /> : <FileUp size={22} aria-hidden />}
            <span>
              <strong>{file ? file.name : "Choose a file"}</strong>
              <small>
                {busy
                  ? "Reading the file…"
                  : file
                    ? `${Math.max(1, Math.round(file.size / 1024))} KB · click to choose another`
                    : "or drop it here · CSV, Excel, OFX, QFX, QIF, CAMT.053 or MT940, or a PDF through the assistant"}
              </small>
            </span>
            {file && (
              <button
                type="button"
                className="icon-button"
                aria-label="Remove the file"
                onClick={(e) => {
                  e.preventDefault();
                  onState({ account });
                  setMessage("");
                }}
              >
                <X size={15} />
              </button>
            )}
            <input
              type="file"
              className="sr-only"
              aria-label="Bank file"
              accept=".csv,.xlsx,.xls,.xlsm,.xlsb,.ods,.ofx,.qfx,.qif,.xml,.sta,.940,.mt940,.zip,.txt,.pdf"
              disabled={busy}
              onChange={(e: ChangeEvent<HTMLInputElement>) => {
                const chosen = e.target.files?.[0];
                if (chosen) void start(chosen);
                e.target.value = "";
              }}
            />
          </label>
          <div className="import-help">
            <p>
              Any bank's <strong>CSV or Excel</strong> file, or its <strong>OFX, QFX, QIF, CAMT.053 or MT940</strong> statement. Lines that
              don't read, or that the importer leaves out (pending payments, another account's, balances), are shown for you to fix or
              include. The account remembers how its file read. A <strong>PDF</strong> statement goes through the assistant.
            </p>
            <button type="button" className="secondary small" onClick={() => void template()}>
              <Download size={14} /> Download a template
            </button>
          </div>
        </>
      ) : !statement ? (
        reading && (
          <p className="help import-wait" role="status">
            <LoaderCircle size={14} className="spin" aria-hidden /> Reading {file?.name}…
          </p>
        )
      ) : state.view === "editor" || (continuing && !reviewed) ? (
        <div className="import-stage" key="editor">
          <StatementGrid
            statement={statement}
            session={session}
            // Until the file reads again after a change, a line's version is the one before it.
            busy={busy || statement.revision !== session.revision}
            assisted={assisted}
            onFix={(f) => void fix(f)}
            checking={continuing}
            onContinue={() => {
              // What went wrong before (a change refused) isn't why the review can't be made.
              setMessage("");
              setContinuing(true);
              onState((was) => ({ ...was, view: "review" }));
            }}
            onAssistant={
              onFixWithAssistant ??
              (onAskAssistant && file ? () => onAskAssistant(file, `${attention} lines don't read.`, accountName) : undefined)
            }
          />
        </div>
      ) : !reviewed ? (
        checking && (
          <p className="help import-wait" role="status">
            <LoaderCircle size={14} className="spin" aria-hidden /> Checking the lines against your budget…
          </p>
        )
      ) : (
        <div className="import-stage" key="review">
          <ImportReview
            budget={budget}
            rows={reviewed.rows}
            busy={busy}
            icon={<FileText size={18} aria-hidden />}
            // The page's heading names the file and its account.
            action={
              <button className="link-button" disabled={busy} onClick={() => onState((was) => ({ ...was, view: "editor" }))}>
                <Table2 size={14} aria-hidden />
                {attention || out
                  ? ` ${[attention && `${attention} need attention`, out && `${out} left out`].filter(Boolean).join(" · ")} · `
                  : " "}
                Check the file
              </button>
            }
            cancelLabel="Back to the file"
            onCancel={() => onState((was) => ({ ...was, view: "editor" }))}
            onImport={(choices, count) => void save(choices, count)}
            // Kept with the import, so the file looked at again (or another page) doesn't undo them.
            choices={state.choices?.session === session.scope.session ? state.choices : NO_CHOICES}
            onChoices={(next) =>
              onState((was) =>
                was.session?.scope.session === session.scope.session ? { ...was, choices: { ...next, session: session.scope.session } } : was,
              )
            }
          />
        </div>
      )}
      {message && (
        <p role="status" className="import-result">
          {message}
        </p>
      )}
      {onAskAssistant && file && (state.view !== "editor" || pdf) && (
        <div className={`import-assist${problem || pdf ? " needed" : ""}`}>
          <MessageSquareText size={16} aria-hidden />
          <p>
            {pdf
              ? "PDF statements need the assistant. It reads the PDF, finds the transactions, checks them against the statement's totals and brings them to this review, where you decide on each row."
              : problem
                ? "The assistant can read this file, find the transactions and ask you about anything unclear."
                : "If something here doesn't look right, the assistant can go through the file with you."}
            <small>Opens the assistant with this file attached. Nothing is sent until you press Send.</small>
          </p>
          <button
            type="button"
            className={problem || pdf ? "primary small" : "secondary small"}
            onClick={() => onAskAssistant(file, problem, accountName)}
          >
            {pdf ? "Read it with the assistant" : "Help with this import"}
          </button>
        </div>
      )}
    </section>
  );
}
