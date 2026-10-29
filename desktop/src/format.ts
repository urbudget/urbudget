// Number, currency and date display for the open budget. Formatting is hand-rolled
// (not Intl per call) because tables format thousands of amounts.
import { today } from "./types";
import { ltr, monthNames, stripIsolates, t, weekdayNames } from "./i18n";
export type NumberFormat = {
  decimal: "." | ",";
  group: "," | "." | " " | "'" | "";
  /** Where displayed amounts show the currency symbol, or whether it is hidden. */
  symbol: "before" | "after" | "none";
};
export type DateFormat = "iso" | "dmy" | "mdy" | "dmmmy";
export const defaultNumberFormat: NumberFormat = { decimal: ".", group: ",", symbol: "after" };
export const dateFormats: Record<DateFormat, string> = {
  iso: "2026-10-15",
  dmy: "15/10/2026",
  mdy: "10/15/2026",
  dmmmy: "15 Oct 2026",
};
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const symbols = new Map<string, string>();
/** The short symbol for an ISO currency code, e.g. "$" for USD; the code when there is none. */
export function currencySymbol(code: string) {
  let symbol = symbols.get(code);
  if (symbol === undefined) {
    try {
      symbol =
        new Intl.NumberFormat("en", { style: "currency", currency: code, currencyDisplay: "narrowSymbol" })
          .formatToParts(0)
          .find((p) => p.type === "currency")?.value ?? code;
    } catch {
      symbol = code;
    }
    symbols.set(code, symbol);
  }
  return symbol;
}
/** Where a currency's symbol goes unless the budget says otherwise: before a sign ($1.00, €1.00), after a code (1.00 SAR). */
export const defaultSymbol = (code: string): NumberFormat["symbol"] =>
  currencySymbol(code) === code ? "after" : "before";
/** The budget's number format, with the settings it leaves out at their defaults for `currency`. */
export const numberFormatFor = (currency: string, saved?: Partial<NumberFormat>): NumberFormat => ({
  ...defaultNumberFormat,
  symbol: defaultSymbol(currency),
  ...saved,
});
/** ISO 4217 codes the runtime knows, for currency pickers. */
export const currencyCodes = (): string[] => {
  try {
    return (Intl as unknown as { supportedValuesOf(k: string): string[] }).supportedValuesOf("currency");
  } catch {
    return ["AED", "EUR", "GBP", "SAR", "USD"];
  }
};

let active = {
  currency: "SAR",
  number: defaultNumberFormat,
  date: "iso" as DateFormat,
};
/**
 * Applies the open budget's display settings. Returns a key that changes when they do,
 * so the app can re-render memoized rows.
 */
export function configureFormat(b: {
  currency: string;
  numberFormat?: Partial<NumberFormat>;
  dateFormat?: DateFormat;
}) {
  active = {
    currency: b.currency,
    number: numberFormatFor(b.currency, b.numberFormat),
    date: b.dateFormat && b.dateFormat in dateFormats ? b.dateFormat : "iso",
  };
  return `${active.currency}|${active.number.decimal}${active.number.group}${active.number.symbol}|${active.date}`;
}
export const activeFormat = () => active;

const grouped = (whole: number, group: string) => {
  const s = String(whole);
  if (!group || s.length < 4) return s;
  let out = "";
  for (let i = 0; i < s.length; i++) {
    if (i && (s.length - i) % 3 === 0) out += group;
    out += s[i];
  }
  return out;
};
/**
 * Minor units (cents) in the budget's number format and currency placement. Never shows "-0.00".
 * `symbol: false` omits currency. Overrides format previews or another currency without changing
 * the open budget's settings; a null currency means the source currency is unknown.
 */
export function money(value: number, options?: {
  symbol?: boolean;
  currency?: string | null;
  numberFormat?: Partial<NumberFormat>;
}) {
  const { decimal, group, symbol } = options?.numberFormat ? { ...active.number, ...options.numberFormat } : active.number;
  const currency = options?.currency === undefined ? active.currency : options.currency;
  const cents = Math.round(Math.abs(value));
  const text = `${value < 0 && cents ? "-" : ""}${grouped(Math.floor(cents / 100), group)}${decimal}${String(cents % 100).padStart(2, "0")}`;
  // In Arabic an amount keeps its left-to-right order: "-1,234.56 SAR", never "SAR 1,234.56-".
  if (options?.symbol === false || symbol === "none" || currency === null) return ltr(text);
  const sign = currencySymbol(currency);
  const prefix = sign === currency ? `${sign} ` : sign;
  return ltr(
    symbol === "before"
      ? text.startsWith("-")
        ? `-${prefix}${text.slice(1)}`
        : `${prefix}${text}`
      : `${text} ${sign}`,
  );
}
/** "2026-10-15" in the budget's date format (whatever the app's language), kept left to right. */
export function formatDate(iso: string) {
  const [y, m, d] = iso.split("-");
  if (!d) return iso;
  switch (active.date) {
    case "dmy":
      return ltr(`${d}/${m}/${y}`);
    case "mdy":
      return ltr(`${m}/${d}/${y}`);
    case "dmmmy":
      return ltr(`${Number(d)} ${MONTHS[Number(m) - 1]} ${y}`);
    default:
      return ltr(iso);
  }
}
const pad2 = (n: number) => String(n).padStart(2, "0");
const isoOf = (y: number, m: number, d: number) => {
  const date = new Date(y, m - 1, d, 12);
  // Reject dates that roll over (31 Feb).
  if (date.getFullYear() !== y || date.getMonth() !== m - 1 || date.getDate() !== d) return null;
  return `${y}-${pad2(m)}-${pad2(d)}`;
};
/** Moves an ISO date by days or months (months clamp to the end of a shorter month). */
export function shiftDate(iso: string, days: number, months = 0) {
  const [y, m, d] = iso.split("-").map(Number);
  const date = new Date(y, m - 1 + months, 1, 12);
  date.setDate(Math.min(d, new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate()) + days);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}
/**
 * A typed date to ISO, read in the budget's date format: "15/10/2026", "15/10", "15 Oct",
 * "Oct 15 26", "2026-10-15", "today", "yesterday", "tomorrow". Missing years come from `base`.
 * Returns null when it isn't a date.
 */
export function parseDate(text: string, base: string, format: DateFormat = active.date): string | null {
  const t = stripIsolates(text).trim().toLowerCase();
  if (!t) return null;
  if (t === "today" || t === "t") return today();
  if (t === "yesterday" || t === "y") return shiftDate(new Date().toLocaleDateString("en-CA"), -1);
  if (t === "tomorrow") return shiftDate(new Date().toLocaleDateString("en-CA"), 1);
  const baseYear = Number(base.slice(0, 4)) || new Date().getFullYear();
  const year = (y: string | undefined) => (!y ? baseYear : y.length <= 2 ? 2000 + Number(y) : Number(y));
  let m = t.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/);
  if (m) return isoOf(Number(m[1]), Number(m[2]), Number(m[3]));
  const month = (name: string) => MONTHS.findIndex((x) => name.startsWith(x.toLowerCase())) + 1;
  m = t.match(/^(\d{1,2})\s*([a-z]{3,})\.?,?\s*(\d{2,4})?$/);
  if (m && month(m[2])) return isoOf(year(m[3]), month(m[2]), Number(m[1]));
  m = t.match(/^([a-z]{3,})\.?\s*(\d{1,2}),?\s*(\d{2,4})?$/);
  if (m && month(m[1])) return isoOf(year(m[3]), month(m[1]), Number(m[2]));
  const parts = t.split(/[\/.\-\s]+/).filter(Boolean);
  if (parts.length < 2 || parts.length > 3 || parts.some((p) => !/^\d+$/.test(p))) return null;
  if (parts[0].length === 4) return isoOf(Number(parts[0]), Number(parts[1]), Number(parts[2] ?? 1));
  const [a, b2, y] = parts;
  return format === "mdy" ? isoOf(year(y), Number(a), Number(b2)) : isoOf(year(y), Number(b2), Number(a));
}
/** "2026-10" to "October 2026" (or "Oct 2026"); Arabic has no short month names. */
export const monthName = (month: string, style: "long" | "short" = "long") => {
  const names = monthNames();
  if (names) return `${names[Number(month.slice(5, 7)) - 1]} ${month.slice(0, 4)}`;
  return new Date(`${month}-02T12:00:00`).toLocaleDateString("en-GB", {
    month: style,
    year: "numeric",
  });
};
/** "2026-10-15" as a heading: "Today", "Yesterday", "Tomorrow", or "Thu, 15 Oct" (with the year when it isn't this one). */
export function dayName(iso: string) {
  const now = today();
  if (iso === now) return t("Today");
  if (iso === shiftDate(now, -1)) return t("Yesterday");
  if (iso === shiftDate(now, 1)) return t("Tomorrow");
  const months = monthNames(), weekdays = weekdayNames();
  if (months && weekdays) {
    const date = new Date(`${iso}T12:00:00`);
    const year = iso.slice(0, 4) === now.slice(0, 4) ? "" : ` ${iso.slice(0, 4)}`;
    return `${weekdays[date.getDay()]}، ${date.getDate()} ${months[date.getMonth()]}${year}`;
  }
  return new Date(`${iso}T12:00:00`).toLocaleDateString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(iso.slice(0, 4) === now.slice(0, 4) ? {} : { year: "numeric" }),
  });
}
/** "2026-10" to "October". */
export const monthOnly = (month: string) =>
  monthNames()?.[Number(month.slice(5, 7)) - 1] ??
  new Date(`${month}-02T12:00:00`).toLocaleDateString("en-GB", { month: "long" });
/** 1 to "1st", 22 to "22nd", 13 to "13th". */
export const ordinal = (n: number) =>
  `${n}${n % 100 >= 11 && n % 100 <= 13 ? "th" : (["th", "st", "nd", "rd"][n % 10] ?? "th")}`;
/** "2026-10-01" as a deadline in a sentence: "October 1st" when dates go month first, else "1 October". */
export const deadline = (iso: string) => {
  const day = Number(iso.slice(8, 10));
  const month = monthOnly(iso.slice(0, 7));
  // Arabic writes the day first whatever the budget's date order: "1 أكتوبر".
  return active.date === "mdy" && !monthNames() ? `${month} ${ordinal(day)}` : `${day} ${month}`;
};
export const shiftMonth =(month: string, delta: number) => {
  const d = new Date(`${month}-02T12:00:00`);
  d.setMonth(d.getMonth() + delta);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
};
/** CSS class for a signed amount. */
export const tone = (value: number) => (value < 0 ? "negative" : value > 0 ? "positive" : "zero");
/** A thrown value as a sentence for people: no "Error:" or internal "CONFLICT:" prefixes. */
export const errorText = (e: unknown) =>
  String(e instanceof Error ? e.message : e)
    .replace(/^(Error: )+/, "")
    .replace(/^CONFLICT: /, "");

/**
 * Minor units as someone would type them into a field being edited: "-1234.50", or "-1234,50"
 * in a budget whose decimal mark is a comma. No grouping, so evaluateAmount reads it back as the
 * same amount whatever the budget's format.
 */
export const editableAmount = (cents: number) => (cents / 100).toFixed(2).replace(".", active.number.decimal);

/** The register's flow entry: a plain +amount in Outflow or -amount in Inflow moves to the other field. */
export function flowAmountChange(field: "outflow" | "inflow", text: string): {
  values: { outflow?: string; inflow?: string };
  focus?: "outflow" | "inflow";
} {
  const other = field === "outflow" ? "inflow" : "outflow";
  const sign = field === "outflow" ? /^\+/ : /^[-−]/;
  if (sign.test(text) && /^[\d\s.,']*$/.test(text.slice(1)))
    return { values: { [field]: "", [other]: text.slice(1) }, focus: other };
  return { values: text ? { [field]: text, [other]: "" } : { [field]: "" } };
}

/**
 * Evaluates an amount a person typed, in cents: "1,234.50", "12 + 3.5", "(10+5)*2", "-20".
 * Literals keep at most two decimals; only multiplication and division round.
 */
export function evaluateAmount(text: string, decimal: "." | "," = active.number.decimal): number {
  let s = stripIsolates(text).replace(/[\s']/g, "");
  if (!s) return 0;
  s = decimal === "." ? s.replace(/,/g, "") : s.replace(/\./g, "").replace(/,/g, ".");
  const tokens = s.match(/\d+(?:\.\d*)?|\.\d+|[-+*/()]|./g) ?? [];
  let i = 0;
  const fail = () => {
    throw new Error(t("Enter an amount like 12.50, or math like 10+2.50."));
  };
  // Values are cents as floats: + and - stay exact; * and / rescale.
  const primary = (): number => {
    const t = tokens[i++];
    if (t === "-") return -primary();
    if (t === "+") return primary();
    if (t === "(") {
      const v = sum();
      if (tokens[i++] !== ")") fail();
      return v;
    }
    if (!t || !/^(\d+(\.\d*)?|\.\d+)$/.test(t)) fail();
    const [whole, fraction = ""] = t.split(".");
    if (fraction.length > 2) fail();
    return Number(whole || "0") * 100 + Number(fraction.padEnd(2, "0"));
  };
  const product = (): number => {
    let v = primary();
    while (tokens[i] === "*" || tokens[i] === "/") {
      const op = tokens[i++];
      const r = primary();
      if (op === "/" && r === 0) fail();
      v = op === "*" ? (v * r) / 100 : (v / r) * 100;
    }
    return v;
  };
  const sum = (): number => {
    let v = product();
    while (tokens[i] === "+" || tokens[i] === "-") v = tokens[i++] === "+" ? v + product() : v - product();
    return v;
  };
  const value = sum();
  if (i !== tokens.length || !Number.isFinite(value)) fail();
  const cents = Math.sign(value) * Math.round(Math.abs(value));
  if (!Number.isSafeInteger(cents) || Math.abs(cents) > 1_000_000_000_000)
    throw new Error(t("Amount is too large."));
  return cents === 0 ? 0 : cents;
}
