// The optional urbudget.app account and end-to-end encrypted sync, as the window sees them.
// Rust does the network, the encryption and the database work.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, desktop, type LibrarySnapshot } from "./bridge";
import type { SyncState } from "./useBudget";

/** A change both devices made differently, which sync asks about (see sync_merge.rs). */
export type SyncConflict = {
  /** What the answer is given under; the same from one sync to the next. */
  key: string;
  kind: "account" | "category" | "transaction" | "assignment" | "schedule" | "payee" | "payeeRule" | "monthNote" | "loan" | "budget" | "setting";
  /** The record's name: a category's, an account's, a transaction's payee. */
  label: string;
  /** What else tells the record apart: a transaction's date, account and amount; an assignment's month. */
  about: Record<string, unknown>;
  /** The field both changed, or null when one side deleted the record and the other edited it. */
  field: string | null;
  /** Each side's value; for a deleted record null, and `{ changed: [fields] }` for the edited one. */
  local: unknown;
  remote: unknown;
};
export type BudgetSync = {
  id: string;
  name: string;
  state: "synced" | "uploaded" | "downloaded" | "merged" | "conflict" | "cloud_only" | "local_only" | "waiting" | "update_app" | "error" | "recovery" | "trashed" | "purged" | "queued" | "signed_out" | "evicted" | "locked" | "paused" | "subscription_required" | "deletion_conflict";
  device: string | null;
  updatedAt: string | null;
  message: string | null;
  /** For "conflict": what to choose. Empty when the versions couldn't be combined at all. */
  conflicts?: SyncConflict[];
  review?: {token: string; version: string; identity: Record<string, string>; conflicts: {key: string; fingerprint: string}[]};
  /** What a merge decided on its own, such as assignments made on both devices being added together. */
  notes?: string[];
};
export type SyncReport = {
  at: string;
  state: "ok" | "signed_out" | "locked" | "subscription_required" | "offline" | "error" | "attention" | "waiting";
  message: string | null;
  budgets: BudgetSync[];
  openChanged: boolean;
  changedBudgets?: { id: string; revision: string }[];
  libraryVersion?: string;
};
export type CloudStatus = {
  /** The signed-in account. */
  email: string | null;
  /** What protects saved secrets (Keychain, Windows encrypted storage), or null when they last until the app closes. */
  credentialStore: string | null;
  sync: { unlocked: boolean; last: SyncReport | null; protocol?: number; enabled?: boolean };
  site: string;
  library?: LibrarySnapshot;
};
export type Overview = {
  user: { id: string; email: string; createdAt: string };
  subscription: { status: string; renewsAt: string | null; endsAt: string | null } | null;
  sync: { active: boolean; budgets: number; bytes: number };
};

export const cloud = {
  status: () => invoke<CloudStatus>("cloud_status"),
  overview: () => invoke<Overview>("account_overview"),
  /** Emails a sign-in code; configured reviewers use the separately supplied access code. */
  start: (email: string) => invoke<{ ok: true; devCode?: string; reviewAccess?: boolean }>("account_start", { email }),
  verify: (email: string, code: string) => invoke<CloudStatus>("account_verify", { email, code }),
  signOut: () => invoke<CloudStatus>("account_sign_out"),
  deleteAccount: (confirm: string) => invoke<CloudStatus>("account_delete", { confirm }),
  /** Opens a page of urbudget.app in the browser, signed in when this computer is. */
  open: (next: string) => invoke<void>("account_open", { next }),
  syncNow: (openClean: boolean) => invoke<SyncReport>("sync_now", { openClean }),
  syncHasKey: () => invoke<boolean>("sync_has_key"),
  syncSetup: (passphrase: string) => invoke<{ created: boolean }>("sync_setup", { passphrase }),
  syncReset: (passphrase: string) => invoke<void>("sync_reset", { passphrase }),
  syncTrack: (ids: string[]) => invoke<void>("sync_track", { ids }),
  syncDownload: (id: string) => invoke<string>("sync_download", { id }),
  /** "merge" merges again with the answers in `choices`; "local" and "cloud" keep one whole version. */
  syncResolve: (id: string, keep: "merge" | "local" | "cloud", openClean: boolean, choices?: Record<string, "local" | "cloud">, review?: BudgetSync["review"]) =>
    invoke<SyncReport>("sync_resolve", { id, keep, openClean, choices: choices ?? null, review: review ?? null }),
  syncRemove: (id: string) => invoke<void>("sync_remove", { id }),
  syncLock: () => invoke<void>("sync_lock"),
};

/** Locked v2 accounts still reconcile catalog, receipts and deletion; uploads require the key. */
export function syncCanReconcile(status: Pick<CloudStatus, "email" | "sync"> | null | undefined) {
  return !!status?.email && (status.sync.protocol === 2 || status.sync.unlocked);
}

/** Evaluate against the currently displayed identity, which may have switched during sync. */
export function syncInvalidatesActive(report: Pick<SyncReport, "openChanged" | "changedBudgets">, id: string, revision: string, activeId?: string | null) {
  if (activeId !== undefined && (activeId ?? "") !== id) return true;
  return report.changedBudgets !== undefined
    ? report.changedBudgets.some(change => change.id === id && (!change.revision || change.revision !== revision))
    : report.openChanged;
}

export const syncCadence = (protocol?: number) => protocol === 2
  ? {poll: 30_000, afterSave: 2_000, focus: 5_000}
  : {poll: 300_000, afterSave: 8_000, focus: 60_000};

/** What sync needs to know about the open budget. */
export type OpenBudget = {
  dirty: () => boolean;
  sync: SyncState;
  operation: string;
  /** Changes with every save. */
  token: string;
  fetchDatabase: () => Promise<void>;
  canAdoptIncoming?: () => boolean;
  budgetId?: string;
};

/**
 * Account and sync state for the whole window. Syncs on start, every few minutes, when
 * the window comes back into focus, and shortly after each save, while signed in with sync
 * unlocked. A downloaded version of the open budget is loaded only when it has no unsaved edits.
 */
export function useCloud(open: OpenBudget) {
  const [status, setStatus] = useState<CloudStatus | null>(null);
  const [syncing, setSyncing] = useState(false);
  const latest = useRef(open);
  latest.current = open;
  const statusRef = useRef(status);
  statusRef.current = status;
  const running = useRef<Promise<SyncReport | null> | null>(null);
  const lastRun = useRef(0);
  const batchStarted = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    if (!desktop) return null;
    const [next, library] = await Promise.all([cloud.status(), api.library()]);
    next.library = library;
    setStatus(next);
    return next;
  }, []);

  const syncNow = useCallback(async (): Promise<SyncReport | null> => {
    const current = statusRef.current;
    if (!desktop || !syncCanReconcile(current)) return null;
    if (running.current) return running.current;
    const work = async () => {
      setSyncing(true);
      lastRun.current = Date.now();
      try {
        const o = latest.current;
        const clean = o.canAdoptIncoming?.() ?? (!o.dirty() && o.sync === "saved" && !o.operation);
        const report = await cloud.syncNow(clean);
        const status = await refresh();
        const now = latest.current;
        if (syncInvalidatesActive(report, now.budgetId ?? "", now.token, status?.library?.activeId) && (now.canAdoptIncoming?.() ?? !now.dirty())) await now.fetchDatabase();
        return report;
      } catch {
        await refresh().catch(() => {});
        return null;
      } finally {
        setSyncing(false);
        running.current = null;
      }
    };
    running.current = work();
    return running.current;
  }, [refresh]);

  useEffect(() => {
    if (!desktop) return;
    void refresh().catch(() => {});
  }, [refresh]);

  // Waits for the open budget to load, so the first sync can also bring in its newer version.
  const ready = syncCanReconcile(status);
  const cadence = syncCadence(status?.sync.protocol);
  useEffect(() => {
    if (!ready) return;
    void syncNow();
    const timer = setInterval(() => void syncNow(), cadence.poll);
    const focus = () => {
      if (Date.now() - lastRun.current > cadence.focus) void syncNow();
    };
    const libraryChanged = () => void syncNow();
    window.addEventListener("urbudget-library-changed", libraryChanged);
    window.addEventListener("focus", focus);
    window.addEventListener("online", focus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("urbudget-library-changed", libraryChanged);
      window.removeEventListener("focus", focus);
      window.removeEventListener("online", focus);
    };
  }, [ready, syncNow, cadence.poll, cadence.focus]);

  // Shortly after each save, so other computers see changes within seconds.
  useEffect(() => {
    if (!ready || !open.token) return;
    void refresh().catch(() => {});
    batchStarted.current ??= Date.now();
    const wait = Math.max(0, Math.min(cadence.afterSave, 15_000 - (Date.now() - batchStarted.current)));
    const timer = setTimeout(() => { batchStarted.current = null; void syncNow(); }, wait);
    return () => clearTimeout(timer);
  }, [ready, open.token, syncNow, cadence.afterSave]);

  return { status, setStatus, refresh, syncNow, syncing, activeBudgetId: open.budgetId, pending: !!status?.library?.entries.some(entry => entry.pending) || open.dirty() || !["saved", "loading"].includes(open.sync) };
}
export type Cloud = Omit<ReturnType<typeof useCloud>, "activeBudgetId" | "pending"> & { activeBudgetId?: string; pending?: boolean };
