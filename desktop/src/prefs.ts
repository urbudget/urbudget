import { useEffect, useState } from "react";

/** App preferences, kept on this computer (not in the budget file) and applied straight away. */
export type Prefs = {
  /** First day of the week in calendars: "auto" follows the system. */
  weekStart: "auto" | "0" | "1" | "6";
  /** The page the app opens on. */
  startPage: "budget" | "transactions";
  /** New transactions start cleared. */
  newCleared: boolean;
  /** Registers list upcoming scheduled transactions at the top. */
  showUpcoming: boolean;
  /** Registers show a running balance (when sorted by date). */
  runningBalance: boolean;
  /** Registers show a Flag column. */
  flagColumn: boolean;
  /** Categories show how much their target still needs. */
  showNeeded: boolean;
  /** The budget summary shows the cash age. */
  showAgeOfMoney: boolean;
  /** Back up the budget when the app closes. */
  backupOnClose: boolean;
  /** How many backups to keep (0 keeps them all). */
  keepBackups: number;
  /** The app's language: "system" follows the operating system's first preferred language. */
  language: "system" | "en" | "ar";
  /** Count this copy as in use on urbudget.app, anonymously, once a day (usage.ts). */
  usageCounts: boolean;
  /** Downloaded updates install by themselves (updates.ts useAutomaticUpdate); off, they wait to
   * be installed by hand. */
  autoUpdate: boolean;
};
export const DEFAULT_PREFS: Prefs = {
  weekStart: "auto",
  startPage: "budget",
  newCleared: false,
  showUpcoming: true,
  runningBalance: true,
  flagColumn: true,
  showNeeded: true,
  showAgeOfMoney: true,
  backupOnClose: false,
  keepBackups: 0,
  language: "system",
  usageCounts: true,
  autoUpdate: true,
};
const KEY = "lb:prefs";
/** The window event `setPref` sends, with the changed key and value as its detail. */
export const PREFS_EVENT = "lb:prefs";
const EVENT = PREFS_EVENT;
const read = (): Prefs => {
  try {
    return { ...DEFAULT_PREFS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return DEFAULT_PREFS;
  }
};
export const getPref = <K extends keyof Prefs>(key: K): Prefs[K] => read()[key];
export function setPref<K extends keyof Prefs>(key: K, value: Prefs[K]) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...read(), [key]: value }));
  } catch {
    // Without storage the preference lasts until the app closes.
  }
  window.dispatchEvent(new CustomEvent(EVENT, { detail: { key, value } }));
}
/** A preference and a setter; every component using it updates when it changes. */
export function usePref<K extends keyof Prefs>(key: K): [Prefs[K], (value: Prefs[K]) => void] {
  const [value, setValue] = useState<Prefs[K]>(() => getPref(key));
  useEffect(() => {
    const changed = (e: Event) => {
      const detail = (e as CustomEvent<{ key: keyof Prefs; value: unknown }>).detail;
      if (detail.key === key) setValue(detail.value as Prefs[K]);
    };
    window.addEventListener(EVENT, changed);
    return () => window.removeEventListener(EVENT, changed);
  }, [key]);
  return [value, (v) => setPref(key, v)];
}
/** Clears the remembered layout: column widths, collapsed groups and sidebar sections. */
export function resetLayout() {
  try {
    for (const key of Object.keys(localStorage))
      if (/^lb:(columns|budget:collapsed|sidebar:sections)/.test(key)) localStorage.removeItem(key);
  } catch {
    // Nothing stored.
  }
}
