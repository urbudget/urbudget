import { useCallback, useSyncExternalStore } from "react";
import type { Attachment } from "./api";

/** A message being put together: its text, the files waiting to go with it and the transactions picked for it. */
export type Composer = { draft: string; attachments: Attachment[]; selected: string[] };

const EMPTY: Composer = { draft: "", attachments: [], selected: [] };
// Kept here rather than in the panel, so closing the panel doesn't throw away a message half
// written. Each budget has its own, for as long as the app is open: a file waiting to be sent
// is in that budget's folder, and is forgotten when the app next starts.
const kept = new Map<string, Composer>();
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
const changed = () => {
  for (const listener of listeners) listener();
};

/** The open budget's message, and a way to change it that still reaches it once the panel has closed (a file that finishes attaching). */
export function useComposer(budgetId: string) {
  const composer = useSyncExternalStore(subscribe, () => kept.get(budgetId) ?? EMPTY);
  const compose = useCallback(
    (change: (was: Composer) => Composer) => {
      kept.set(budgetId, change(kept.get(budgetId) ?? EMPTY));
      changed();
    },
    [budgetId],
  );
  return [composer, compose] as const;
}

/** Whether the budget has a message waiting unsent, its panel open or closed: closing the app would lose it. */
export function hasUnsent(budgetId: string) {
  const composer = kept.get(budgetId);
  return !!composer && (composer.draft.trim() !== "" || composer.attachments.length > 0);
}

/** Forgets every message being written, so each test starts with an empty box. */
export function forgetComposers() {
  kept.clear();
  changed();
}
