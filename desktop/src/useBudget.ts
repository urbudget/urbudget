import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getPref } from "./prefs";
import { listen } from "@tauri-apps/api/event";
import { produce, setAutoFreeze } from "immer";
import { api, desktop } from "./bridge";
import { useCalculationMonth } from "./useCalculationMonth";
import {
  type Budget,
  type DatabaseStatus,
  type Summary,
  type YnabImportRequest,
  scheduledAhead,
  today,
} from "./types";

// Budgets are plain objects shared with components that are free to read them.
setAutoFreeze(false);

export type SyncState =
  "loading" | "saved" | "pending" | "saving" | "conflict" | "error";
const LEGACY_RECOVERY = "local-budget-recovery-v1";
type Recovery = { budget: Budget; matches: boolean };
const RECOVERY_ERROR =
  "The recovery draft could not be written. Keep this window open until the database save succeeds.";
const NOT_SAVED_ON_CLOSE =
  "The window is staying open because changes are not saved. Resolve the database conflict or export your draft first.";
/** How long closing waits for Windows to install a Store update before the window goes anyway
 * (the update installs another time). */
const EXIT_UPDATE_WAIT = 2 * 60_000;
/** The operation is already in the draft. Retrying must save it, never apply it again. */
export class AppliedChangeError extends Error {
  constructor(readonly retrySave: () => Promise<void>) {
    super("The changes are already applied to your draft but could not be saved. Retry saving or use database status; do not repeat the operation.");
    this.name = "AppliedChangeError";
  }
}
export function useBudget(month: string, editorOpen = false) {
  const [budget, setBudget] = useState<Budget | null>(null);
  const calculationMonth = useCalculationMonth();
  const [calculationAttempt, setCalculationAttempt] = useState(0);
  const [calculated, setCalculated] = useState<{ budget: Budget; month: string; asOfMonth: string; attempt: number; summary: Summary } | null>(null);
  const [calculationFailure, setCalculationFailure] = useState<{ budget: Budget; month: string; asOfMonth: string; attempt: number; message: string } | null>(null);
  const sameContext = calculated?.month === month && calculated.asOfMonth === calculationMonth;
  const calculationError = calculationFailure?.budget === budget && calculationFailure?.month === month && calculationFailure.asOfMonth === calculationMonth && calculationFailure.attempt === calculationAttempt
    ? calculationFailure.message : "";
  const summary = !calculationError && sameContext && calculated?.budget === budget && calculated.attempt === calculationAttempt ? calculated.summary : null;
  // App can keep a same-month editor mounted while the replacement is pending, but must
  // make that display inert. A failure or calendar/viewed-month change hides old figures.
  const displaySummary = !calculationError && sameContext ? calculated.summary : null;
  const [sync, setSync] = useState<SyncState>("loading");
  const [recoveryReady, setRecoveryReady] = useState(false);
  const [operation, setOperation] = useState("");
  const operationRef = useRef(false);
  const updateReadyToClose = useRef(false);
  /** Runs after everything is saved when the window closes, such as installing an update. */
  const exitTask = useRef<(() => Promise<unknown>) | null>(null);
  const [error, setError] = useState("");
  const [metadata, setMetadata] = useState<DatabaseStatus | null>(null);
  const [recovery, setRecovery] = useState<Recovery | null>(null);
  // Edits use structural sharing, so the draft differs from the saved budget
  // exactly when it is a different object. Undo history shares records too.
  const current = useRef<Budget | null>(null);
  const saved = useRef<Budget | null>(null);
  const past = useRef<Budget[]>([]);
  const future = useRef<Budget[]>([]);
  const [history, setHistory] = useState({ undo: false, redo: false });
  const updateHistory = () =>
    setHistory({
      undo: past.current.length > 0,
      redo: future.current.length > 0,
    });
  const rememberHistory = () => {
    if (current.current)
      past.current = [...past.current.slice(-29), current.current];
    future.current = [];
    updateHistory();
  };
  const editing = useRef(editorOpen);
  editing.current = editorOpen;
  const token = useRef("");
  const blocked = useRef(false);
  const pendingRecovery = useRef(false);
  const running = useRef<Promise<boolean> | null>(null);
  // Future-dated transactions already offered to the scheduler this session (see `post`).
  const askedAhead = useRef(new Set<string>());
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const dirty = () => !!current.current && current.current !== saved.current;

  /** Sends the draft to Rust, which keeps its recovery copy. */
  const push = (next: Budget) =>
    api
      .push(next)
      .then((r) => {
        if (r.recoveryError) setError(RECOVERY_ERROR);
      })
      .catch(() => setError(RECOVERY_ERROR));
  const accept = useCallback((b: Budget, status: DatabaseStatus) => {
    past.current = [];
    future.current = [];
    updateHistory();
    current.current = b;
    saved.current = b;
    token.current = status.token;
    setBudget(b);
    setMetadata(status);
    blocked.current = false;
    setSync("saved");
    setError("");
  }, []);

  const flush = useCallback(async (): Promise<boolean> => {
    clearTimeout(timer.current);
    if (running.current) return running.current;
    if (blocked.current || pendingRecovery.current) return false;
    const work = async () => {
      try {
        if (!dirty() && current.current) {
          const status = await api.status();
          if (status.token !== token.current)
            throw new Error(
              "CONFLICT: The database changed. Fetch the saved version before continuing.",
            );
          setMetadata(status);
        }
        while (dirty() && current.current) {
          setSync("saving");
          const snapshot = current.current;
          await api.push(snapshot);
          const status = await api.commit(token.current);
          // Edits made meanwhile were queued behind this save and are already
          // in the draft; the loop saves them next.
          saved.current = snapshot;
          token.current = status.token;
          setMetadata(status);
        }
        setSync("saved");
        setError("");
        return true;
      } catch (e) {
        const message = String(e);
        blocked.current = message.includes("CONFLICT:");
        setSync(blocked.current ? "conflict" : "error");
        setError(message.replace(/^(Error: )?CONFLICT: /, ""));
        return false;
      }
    };
    running.current = work();
    const result = await running.current;
    running.current = null;
    return result;
  }, []);

  const change = (next: Budget) => {
    current.current = next;
    setBudget(next);
    void push(next);
    if (!blocked.current) {
      setSync("pending");
      clearTimeout(timer.current);
      timer.current = setTimeout(() => void flush(), 400);
    }
  };
  const edit = useCallback(
    (update: (draft: Budget) => void) => {
      if (!current.current || pendingRecovery.current || operationRef.current)
        return;
      const next = produce(current.current, update);
      if (next === current.current) return;
      rememberHistory();
      change(next);
    },
    [flush],
  );
  /** Like `edit`, but not undoable: setup the user didn't do by hand. */
  const settle = useCallback(
    (update: (draft: Budget) => void) => {
      if (!current.current || pendingRecovery.current || operationRef.current) return;
      const next = produce(current.current, update);
      if (next !== current.current) change(next);
    },
    [flush],
  );

  const travel = useCallback(
    (redo = false) => {
      if (
        !current.current ||
        pendingRecovery.current ||
        operationRef.current ||
        blocked.current
      )
        return;
      const from = redo ? future : past,
        to = redo ? past : future;
      const next = from.current.pop();
      if (!next) return;
      to.current.push(current.current);
      updateHistory();
      change(next);
    },
    [flush],
  );
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        editing.current ||
        target.closest?.("input,textarea,select,[contenteditable=true]") ||
        !(e.metaKey || e.ctrlKey)
      )
        return;
      if (
        e.key.toLowerCase() === "z" ||
        (e.ctrlKey && e.key.toLowerCase() === "y")
      ) {
        e.preventDefault();
        travel(e.shiftKey || e.key.toLowerCase() === "y");
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [travel]);

  const forgetRecovery = async () => {
    await api.discardRecovery();
    localStorage.removeItem(LEGACY_RECOVERY);
  };
  const fetchDatabase = useCallback(async () => {
    clearTimeout(timer.current);
    if (running.current) await running.current;
    const draftAtStart = current.current;
    try {
      const data = await api.load();
      if (current.current !== draftAtStart) {
        setError(
          "You made another edit while the database was being fetched. Your edit has been kept; fetch again when ready.",
        );
        void push(current.current!);
        return;
      }
      accept(data.budget, data);
      await forgetRecovery();
      setRecovery(null);
      pendingRecovery.current = false;
    } catch (e) {
      setError(String(e));
      setSync("error");
    }
  }, [accept]);

  useEffect(() => {
    if (!desktop) {
      setSync("error");
      setError(
        "Open the desktop app to use the Rust database. Run npm run desktop from the desktop folder.",
      );
      return;
    }
    let cancelled = false;
    void api
      .load()
      .then(async (data) => {
        if (cancelled) return;
        pendingRecovery.current = true;
        accept(data.budget, data);
        try {
          const nativeDraft = await api.recoveryRead();
          if (cancelled) return;
          const raw = localStorage.getItem(LEGACY_RECOVERY);
          const draft =
            nativeDraft ??
            (raw
              ? { budget: (JSON.parse(raw) as Recovery).budget, matches: false }
              : null);
          pendingRecovery.current = !!draft;
          if (draft) setRecovery(draft);
        } catch {
          pendingRecovery.current = false;
          setError(
            "An unreadable recovery draft was found. The saved database has been loaded.",
          );
        }
        setRecoveryReady(true);
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setSync("error");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [accept]);

  useEffect(() => {
    if (!budget) return;
    const controller = new AbortController();
    void api
      .calculate(budget, month, { asOfMonth: calculationMonth, signal: controller.signal })
      .then((s) => {
        if (!controller.signal.aborted) {
          setCalculated({ budget, month, asOfMonth: calculationMonth, attempt: calculationAttempt, summary: s });
          setCalculationFailure(null);
        }
      })
      .catch((e) => {
        if (!controller.signal.aborted)
          setCalculationFailure({ budget, month, asOfMonth: calculationMonth, attempt: calculationAttempt, message: String(e) });
      });
    return () => {
      controller.abort();
    };
  }, [budget, month, calculationMonth, calculationAttempt]);

  useEffect(() => {
    if (!desktop) return;
    const check = async () => {
      if (
        !current.current ||
        running.current ||
        pendingRecovery.current ||
        operationRef.current
      )
        return;
      try {
        const expected = token.current;
        const status = await api.status();
        // A save/import can start while the status request is in flight. Adopting
        // another writer's token here would let its old snapshot save over that
        // writer without the conflict check noticing.
        if (running.current || operationRef.current || pendingRecovery.current || expected !== token.current) return;
        if (status.token !== token.current) {
          if (dirty() || editing.current) {
            blocked.current = true;
            clearTimeout(timer.current);
            setSync("conflict");
            setError(
              "The database changed while you were editing. Export your draft, or fetch the saved version.",
            );
          } else {
            const data = await api.load();
            if (
              !dirty() &&
              !editing.current &&
              !running.current &&
              !operationRef.current &&
              !pendingRecovery.current &&
              expected === token.current
            )
              accept(data.budget, data);
            else if (current.current) void push(current.current);
          }
        }
      } catch (e) {
        setError(String(e));
        setSync("error");
      }
    };
    const interval = setInterval(() => void check(), 3000);
    window.addEventListener("focus", check);
    return () => {
      clearInterval(interval);
      window.removeEventListener("focus", check);
    };
  }, [accept]);

  /** The on-close backup, when it's turned on. The native store applies the saved retention
   * policy whenever it creates a backup; a failed one never keeps the window open. */
  const backupOnClose = async () => {
    if (!getPref("backupOnClose")) return;
    try {
      await api.backup();
    } catch {
      // Best effort: saving is what closing needs.
    }
  };
  /** What quitting needs first, for closing the window or installing an update: everything
   * saved, then the on-close backup. False when the changes can't be saved. */
  const prepareExit = useCallback(async (): Promise<boolean> => {
    if (operationRef.current || !(await flush())) return false;
    await backupOnClose();
    return true;
  }, [flush]);

  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        // An installer may close the app after saving. Edits remain locked, so
        // don't veto that shutdown or start a second backup during deployment.
        if (updateReadyToClose.current) return;
        if (operationRef.current) {
          event.preventDefault();
          return;
        }
        // An update waiting to install on close, unless a recovery draft is waiting too.
        const task = pendingRecovery.current ? null : exitTask.current;
        if (task) {
          event.preventDefault();
          // As installing by hand does, it holds the edit lock from the save until the window
          // goes, so nothing changes after it's saved, and Windows (or a second close) may end
          // the app only while it's held. Closing never waits long on the update, nor on its failing.
          let saved = false;
          try {
            await exclusive("Installing the update…", async () => {
              saved = true;
              await backupOnClose();
              updateReadyToClose.current = true;
              await Promise.race([task().catch(() => {}), new Promise((done) => setTimeout(done, EXIT_UPDATE_WAIT))]);
              await getCurrentWindow().destroy();
            });
          } catch {
            if (!saved) setError(NOT_SAVED_ON_CLOSE);
          } finally {
            updateReadyToClose.current = false;
          }
          return;
        }
        if (pendingRecovery.current || dirty() || running.current || getPref("backupOnClose")) {
          event.preventDefault();
          if (await prepareExit()) await getCurrentWindow().destroy();
          else setError(NOT_SAVED_ON_CLOSE);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      });
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        void flush();
      }
    };
    window.addEventListener("keydown", key);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("keydown", key);
    };
  }, [flush, prepareExit]);

  const exclusive = async <T>(
    label: string,
    work: () => Promise<T>,
  ): Promise<T> => {
    if (operationRef.current || pendingRecovery.current)
      throw new Error("Finish the current operation or recovery first.");
    operationRef.current = true;
    setOperation(label);
    try {
      if (!(await flush()))
        throw new Error("Resolve unsaved changes before continuing.");
      return await work();
    } finally {
      operationRef.current = false;
      setOperation("");
    }
  };
  const importYnab = (request: YnabImportRequest) =>
    exclusive("Importing from YNAB…", async () => {
      const unlisten = await listen<string>("ynab-import-progress", (e) =>
        setOperation(e.payload),
      );
      try {
        const result = await api.syncYnab(request, token.current);
        if (result.reviewId) return result;
        accept(result.snapshot.budget, result.snapshot);
        await forgetRecovery();
        return result;
      } finally {
        unlisten();
      }
    });
  const acceptYnab = (reviewId: string) =>
    exclusive("Saving the import…", async () => {
      const result = await api.acceptYnab(reviewId, token.current);
      if (result.reviewId) return result;
      accept(result.snapshot.budget, result.snapshot);
      await forgetRecovery();
      return result;
    });
  const resolveYnab = (reviewId: string, choices: Record<string, string>) =>
    exclusive("Applying reviewed import…", async () => {
      const result = await api.resolveYnab(reviewId, choices, token.current);
      accept(result.snapshot.budget, result.snapshot);
      await forgetRecovery();
      return result;
    });
  const restoreBackup = (budget: Budget) =>
    exclusive("Restoring backup…", async () => {
      const snapshot = await api.restoreBackup(budget, token.current);
      accept(snapshot.budget, snapshot);
      await forgetRecovery();
    });
  const transform = (work: (budget: Budget) => Promise<Budget>) =>
    exclusive("Saving changes…", async () => {
      const next = await work(current.current!);
      await api.validate(next);
      rememberHistory();
      current.current = next;
      setBudget(next);
      await push(next);
      if (!(await flush()))
        throw new AppliedChangeError(() => exclusive("Saving changes…", async () => {}));
    });
  useEffect(() => {
    if (!desktop) return;
    const post = async () => {
      // Transactions entered for a day ahead are scheduled ones: Rust moves them to Scheduled
      // (schedule_future), except imported or cleared ones, which this asks about only once.
      const ahead =
        current.current?.transactions.filter((t) => scheduledAhead(t) && !askedAhead.current.has(t.id)) ?? [];
      if (
        !current.current ||
        !recoveryReady ||
        (!ahead.length &&
          !current.current.schedules?.some(
            (s) => !s.paused && s.nextDate && s.nextDate <= today(),
          )) ||
        dirty() ||
        editing.current ||
        pendingRecovery.current ||
        operationRef.current ||
        running.current
      )
        return;
      operationRef.current = true;
      setOperation("Posting scheduled transactions…");
      try {
        const data = await api.runSchedules(token.current, today());
        for (const t of ahead) askedAhead.current.add(t.id);
        accept(data.budget, data);
      } catch (e) {
        setError(String(e));
      } finally {
        operationRef.current = false;
        setOperation("");
      }
    };
    const interval = setInterval(() => void post(), 60000);
    void post();
    window.addEventListener("focus", post);
    return () => {
      clearInterval(interval);
      window.removeEventListener("focus", post);
    };
  }, [budget, accept, recoveryReady]);

  /** Saves the open budget, then opens another one and offers its recovery file, if any. */
  const switchBudget = (id: string) =>
    exclusive("Opening budget…", async () => {
      const data = await api.openBudget(id);
      setCalculated(null);
      accept(data.budget, data);
      setRecovery(null);
      pendingRecovery.current = false;
      const draft = await api.recoveryRead().catch(() => null);
      if (draft) {
        pendingRecovery.current = true;
        setRecovery(draft);
      }
    });
  const restoreRecovery = async () => {
    if (!recovery) return;
    await api.validate(recovery.budget);
    current.current = recovery.budget;
    setBudget(recovery.budget);
    setRecovery(null);
    pendingRecovery.current = false;
    await push(recovery.budget);
    localStorage.removeItem(LEGACY_RECOVERY);
    if (!recovery.matches) {
      blocked.current = true;
      setSync("conflict");
      setError(
        "The recovered draft was based on an older database. Export it for comparison, or fetch the database.",
      );
    } else {
      setSync("pending");
      await flush();
    }
  };
  return {
    budget,
    /** The first load has finished looking for a recovery draft, so edits can go through. */
    ready: recoveryReady,
    /** Id of the open budget (the first part of the save token). */
    budgetId: metadata?.token.split(":")[0] ?? "",
    switchBudget,
    dismissError: () => setError(""),
    canUndo: history.undo,
    canRedo: history.redo,
    undo: () => travel(),
    redo: () => travel(true),
    operation,
    importYnab,
    acceptYnab,
    resolveYnab,
    restoreBackup,
    transform,
    installUpdate: (install: () => Promise<void>) =>
      exclusive("Installing the update…", async () => {
        await backupOnClose();
        updateReadyToClose.current = true;
        try { await install(); } finally { updateReadyToClose.current = false; }
      }),
    summary,
    displaySummary,
    calculationMonth,
    calculationError,
    retryCalculation: () => setCalculationAttempt((n) => n + 1),
    sync,
    error,
    metadata,
    recovery,
    edit,
    settle,
    flush,
    prepareExit,
    /** Sets what closing the window does once everything is saved (null for nothing). */
    setExitTask: (task: (() => Promise<unknown>) | null) => {
      exitTask.current = task;
    },
    fetchDatabase,
    restoreRecovery,
    dirty,
  };
}
