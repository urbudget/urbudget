import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { desktop } from "./bridge";

export type EditorDraft = { budgetId: string; editorId: string; epoch: string; target: unknown; baseline: unknown; draft: unknown };
export const editorDrafts = (budgetId?: string) => invoke<EditorDraft[]>("editor_recovery", { budgetId: budgetId ?? null });
export const discardEditorDraft = (draft: EditorDraft) => invoke("editor_discard", { budgetId: draft.budgetId, id: draft.editorId, epoch: draft.epoch });

const pendingCheckpoints = new Set<EditorCheckpoint>();
/** Includes leaf editors outside App's dialog state; the close path bounds this drain. */
export async function flushAllEditorDrafts() {
  while (pendingCheckpoints.size) await Promise.all([...pendingCheckpoints].map(checkpoint => checkpoint.flush()));
}

/** Serial writes make a delayed checkpoint unable to recreate an explicitly closed draft. */
export class EditorCheckpoint {
  readonly id = crypto.randomUUID();
  epoch = 0;
  private lane: Promise<unknown> = Promise.resolve();
  private previous = "";
  private failed = false;
  constructor(readonly budgetId: string, readonly target: unknown, readonly baseline: unknown, private failure: (error: unknown) => void) {}
  private track() {
    pendingCheckpoints.add(this);
    const tail = this.lane;
    void tail.finally(() => { if (this.lane === tail) pendingCheckpoints.delete(this); });
  }
  save(values: unknown) {
    const encoded = JSON.stringify(values);
    if (encoded === this.previous) return;
    this.previous = encoded;
    const epoch = ++this.epoch;
    this.lane = this.lane.catch(() => {}).then(() => epoch === this.epoch ? invoke("editor_checkpoint", {
      budgetId: this.budgetId, id: this.id, epoch: String(epoch), target: this.target, baseline: this.baseline, values: JSON.parse(encoded),
    }).then(() => { this.failed = false; }) : undefined).catch(error => { this.failed = true; this.failure(error); });
    this.track();
  }
  discard(onlyIfActive = false) {
    const epoch = this.epoch;
    this.lane = this.lane.catch(() => {}).then(() => onlyIfActive && this.failed ? undefined : invoke("editor_discard", { budgetId: this.budgetId, id: this.id, epoch: String(epoch), ...(onlyIfActive ? {onlyIfActive: true} : {}) })).catch(this.failure);
    this.track();
  }
  flush() { return this.lane; }
}

/** Retains raw fields; normal unmount clears active editors, while removed identities keep recovery. */
export function useEditorCheckpoint(budgetId: string, kind: string | null, budget: unknown, failure: (error: unknown) => void, values?: unknown, selector = '[aria-modal="true"] input, [aria-modal="true"] textarea, [aria-modal="true"] select') {
  const session = useRef<EditorCheckpoint | null>(null);
  const baseline = useRef(budget); baseline.current = budget;
  const fail = useRef(failure); fail.current = failure;
  const raw = useRef(values); raw.current = values;
  const captureNow = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (!budgetId || !desktop) return;
    if (!kind || !budgetId) {
      if (!kind && session.current?.budgetId === budgetId) { session.current.discard(); session.current = null; }
      return;
    }
    // A different identity leaves the former draft in recovery, even if React removes its form.
    if (!session.current || session.current.budgetId !== budgetId || session.current.target !== kind) {
      if (session.current?.budgetId === budgetId) session.current.discard(true);
      session.current = new EditorCheckpoint(budgetId, kind, baseline.current, error => fail.current(error));
    }
    const owned = session.current;
    let active = true;
    let lastFields: unknown[] = [];
    const capture = () => {
      if (!active) return;
      const fields = Array.from(document.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(
        selector,
      )).filter(field => field.type !== "password" && field.autocomplete !== "one-time-code").map((field, index) => ({
        index, label: field.getAttribute("aria-label") || field.labels?.[0]?.textContent || field.name || field.id,
        value: field.value, checked: field instanceof HTMLInputElement ? field.checked : undefined,
      }));
      if (fields.length) lastFields = fields;
      if (fields.length || owned.epoch === 0 || raw.current !== undefined) owned.save({ fields: fields.length ? fields : lastFields, state: raw.current });
    };
    captureNow.current = capture;
    capture();
    // Clicks include keypad and custom choice controls whose values are rendered after the event.
    const afterEvent = () => queueMicrotask(capture);
    document.addEventListener("input", afterEvent);
    document.addEventListener("change", afterEvent);
    document.addEventListener("click", afterEvent);
    return () => {
      active = false;
      document.removeEventListener("input", afterEvent);
      document.removeEventListener("change", afterEvent);
      document.removeEventListener("click", afterEvent);
      // A StrictMode restart installs a new capture before this microtask. Real unmounts
      // clean up only while the same budget is still active; deletions retain recovery.
      queueMicrotask(() => {
        if (captureNow.current === capture) { captureNow.current = null; owned.discard(true); }
      });
    };
  }, [budgetId, kind, selector]);
  useEffect(() => { captureNow.current?.(); }, [values]);
  const flush = () => session.current?.flush() ?? Promise.resolve();
  flush.discard = () => session.current?.discard();
  return flush;
}
