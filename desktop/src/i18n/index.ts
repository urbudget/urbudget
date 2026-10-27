// The window's language: English (the source, written in the code) or Arabic (ar.ts).
//
// Strings are their own keys: `t("Save")`, `t("Version {version} is ready", { version })`, and
// `tn(count, "{count} transaction", "{count} transactions")` for a count. English is the key
// itself, so a string missing from a catalog shows in English. docs/LOCALIZATION.md says what is
// translated and how.
import { createElement, Fragment, useSyncExternalStore, type ReactNode } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getPref, PREFS_EVENT } from "../prefs";
import { ar } from "./ar";

export type Language = "en" | "ar";
export type LanguageSetting = "system" | Language;
/** A count's forms, by the language's plural categories (Arabic uses all six). */
export type PluralForms = Record<"zero" | "one" | "two" | "few" | "many" | "other", string>;
/** English source strings to their translation. Plural entries are keyed by the English "other" form. */
export type Catalog = Record<string, string | PluralForms>;
/** Values for a string's `{placeholders}`. */
export type Values = Record<string, string | number>;
/** Values that include React elements, such as a bold name or a link. */
export type RichValues = Record<string, ReactNode>;

const catalogs: Record<Language, Catalog | null> = { en: null, ar };

/** The languages the operating system prefers, most preferred first. */
export function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages?.length ? navigator.languages : navigator.language ? [navigator.language] : [];
}

/** The language a setting means: System is Arabic when the system's first language is ("ar", "ar-SA"…). */
export function resolveLanguage(setting: LanguageSetting, languages: readonly string[] = systemLanguages()): Language {
  if (setting === "en" || setting === "ar") return setting;
  return /^ar(?:[-_]|$)/i.test(languages[0] ?? "") ? "ar" : "en";
}

const setting = (): LanguageSetting => {
  const value = getPref("language");
  return value === "en" || value === "ar" ? value : "system";
};
let current: Language = resolveLanguage(setting());
const listeners = new Set<() => void>();

/** The language in use now. */
export const getLanguage = () => current;
/** Text direction for the language in use. */
export const getDirection = () => (current === "ar" ? "rtl" : "ltr");

/** Points the document and the native side at the language in use. */
export function applyLanguage() {
  if (typeof document !== "undefined") {
    document.documentElement.lang = current;
    document.documentElement.dir = getDirection();
  }
  // The engine writes some messages itself (errors, sync and import status).
  try {
    if (isTauri()) void invoke("set_language", { language: current }).catch(() => {});
  } catch {
    // An older native side without the command: its messages stay in English.
  }
}

function refresh() {
  const next = resolveLanguage(setting());
  if (next === current) return;
  current = next;
  applyLanguage();
  for (const listener of listeners) listener();
}
if (typeof window !== "undefined") {
  window.addEventListener(PREFS_EVENT, (e) => {
    if ((e as CustomEvent<{ key: string }>).detail?.key === "language") refresh();
  });
  // System follows the operating system when its languages change.
  window.addEventListener("languagechange", refresh);
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};
/**
 * The language in use; the component renders again when it changes. App uses it, so the whole
 * window follows; memoized components and memoized text use it too.
 */
export const useLanguage = () => useSyncExternalStore(subscribe, getLanguage, getLanguage);

// Arabic keeps names, amounts, dates and emails in their own direction: each value placed in a
// sentence is isolated (First Strong Isolate … Pop Directional Isolate).
const FSI = "\u2068";
const PDI = "\u2069";

function fill(text: string, values: RichValues | undefined): ReactNode {
  if (!values) return text;
  const isolate = current === "ar";
  const parts = text.split(/\{(\w+)\}/);
  let rich = false;
  const out: ReactNode[] = [];
  parts.forEach((part, i) => {
    if (i % 2 === 0) {
      if (part) out.push(part);
      return;
    }
    if (!(part in values)) {
      out.push(`{${part}}`);
      return;
    }
    const value = values[part];
    if (typeof value === "string" || typeof value === "number") {
      out.push(isolate ? FSI + value + PDI : String(value));
      return;
    }
    rich = true;
    if (isolate) out.push(FSI, value, PDI);
    else out.push(value);
  });
  if (!rich) return out.join("");
  return createElement(Fragment, null, ...out);
}

// A key with a context (see `tc`) is the English text, this separator and the context; its
// catalog entry is "text (context)". The separator never appears in real text.
const CONTEXT = "\u0004";
const catalogKey = (raw: string) => {
  const at = raw.indexOf(CONTEXT);
  return at < 0 ? { text: raw, key: raw } : { text: raw.slice(0, at), key: `${raw.slice(0, at)} (${raw.slice(at + 1)})` };
};
const lookup = (raw: string) => {
  const { text, key } = catalogKey(raw);
  const entry = catalogs[current]?.[key];
  return typeof entry === "string" && entry ? entry : text;
};

/**
 * The English `key` in the language in use, with its `{placeholders}` filled. With React
 * elements among the values the result is a fragment; otherwise it is a string.
 */
export function t(key: string, values?: Values): string;
export function t(key: string, values: RichValues): ReactNode;
export function t(key: string, values?: RichValues): ReactNode {
  return fill(lookup(key), values);
}

const rules = new Map<Language, Intl.PluralRules>();
const pluralRules = (language: Language) => {
  let r = rules.get(language);
  if (!r) rules.set(language, (r = new Intl.PluralRules(language)));
  return r;
};

/**
 * A string for a count: `tn(n, "{count} transaction", "{count} transactions")`. English picks
 * `one` for 1 and `other` otherwise; Arabic picks among zero, one, two, few, many and other.
 * `{count}` is the count unless `values` gives it (say, formatted as money).
 */
export function tn(count: number, one: string, other: string, values?: Values): string;
export function tn(count: number, one: string, other: string, values: RichValues): ReactNode;
export function tn(count: number, one: string, other: string, values?: RichValues): ReactNode {
  const { text: english, key } = catalogKey(other);
  const entry = catalogs[current]?.[key];
  const category = pluralRules(current).select(count) as keyof PluralForms;
  const text =
    entry && typeof entry === "object" ? entry[category] || entry.other : category === "one" ? catalogKey(one).text : english;
  return fill(text, { count, ...values });
}

/**
 * The same English with another meaning, told apart by a context: `tc("noun", "Report")` is
 * "Report" in English, and its catalog entry is "Report (noun)".
 */
export function tc(context: string, key: string, values?: Values): string;
export function tc(context: string, key: string, values: RichValues): ReactNode;
export function tc(context: string, key: string, values?: RichValues): ReactNode {
  return fill(lookup(msgc(context, key)), values);
}

/** `tn` for a count whose English has another meaning elsewhere; its catalog entry is "other (context)". */
export function tnc(context: string, count: number, one: string, other: string, values?: Values): string;
export function tnc(context: string, count: number, one: string, other: string, values: RichValues): ReactNode;
export function tnc(context: string, count: number, one: string, other: string, values?: RichValues): ReactNode {
  return tn(count, one, msgc(context, other), values as RichValues);
}

/** `msg` with a context, for a table: translate the value later with `t(value)`. */
export const msgc = (context: string, key: string) => key + CONTEXT + context;

/**
 * Marks an English string for translation where `t` can't run yet, such as a table defined
 * when the module loads; translate it with `t(value)` when rendering.
 */
export const msg = <K extends string>(key: K): K => key;

/** Items as a list with commas, "a, b, c" (in Arabic «a، b، c»), with no "and". */
export const list = (items: readonly string[]) => items.join(current === "ar" ? "، " : ", ");

// Arabic text uses Western digits and the Gregorian calendar, whatever the system's region.
const AR_LOCALE = "ar-u-nu-latn-ca-gregory";
/** A count with digit grouping: as the system writes it, or with Western digits in Arabic. */
export const formatCount = (n: number) => (current === "ar" ? n.toLocaleString("en-US") : n.toLocaleString());
/** A moment's date in the system's style; in Arabic, with Western digits and Gregorian months. */
export const localDate = (date: Date, options?: Intl.DateTimeFormatOptions) =>
  current === "ar" ? date.toLocaleDateString(AR_LOCALE, options) : date.toLocaleDateString(undefined, options);
/** A moment's date and time in the system's style; in Arabic, with Western digits and Gregorian months. */
export const localDateTime = (date: Date, options?: Intl.DateTimeFormatOptions) =>
  current === "ar" ? date.toLocaleString(AR_LOCALE, options) : date.toLocaleString(undefined, options);

// Left-to-right isolate, for text that must never be reordered inside Arabic (amounts, dates).
const LRI = "\u2066";
/** In Arabic, `text` isolated as right to left, for a sentence inside a left-to-right layout (a chart). */
export const rtlRun = (text: string) => (current === "ar" ? "\u2067" + text + PDI : text);
/** In Arabic, `text` isolated as left to right; in English, unchanged. */
export const ltr = (text: string) => (current === "ar" ? LRI + text + PDI : text);
/** Removes the isolation marks `ltr` and `t` add, for text read back as input. */
export const stripIsolates = (text: string) => text.replace(/[\u2066-\u2069\u200e\u200f]/g, "");

/** Month names, January first, in the language in use. */
export const monthNames = (): readonly string[] | null => (current === "ar" ? AR_MONTHS : null);
/** Weekday names, Sunday first, in the language in use (null for English, which uses Intl). */
export const weekdayNames = (): readonly string[] | null => (current === "ar" ? AR_WEEKDAYS : null);

// The Gregorian months as Arabic software in Egypt and the Gulf names them, and the weekdays.
const AR_MONTHS = [
  "يناير",
  "فبراير",
  "مارس",
  "أبريل",
  "مايو",
  "يونيو",
  "يوليو",
  "أغسطس",
  "سبتمبر",
  "أكتوبر",
  "نوفمبر",
  "ديسمبر",
] as const;
const AR_WEEKDAYS = ["الأحد", "الاثنين", "الثلاثاء", "الأربعاء", "الخميس", "الجمعة", "السبت"] as const;
