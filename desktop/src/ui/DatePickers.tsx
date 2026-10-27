import { useRef, useState } from "react";
import { CalendarDays, ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import { Popover } from "./Popover";
import { DateInput } from "./DateInput";
import { formatDate, monthName } from "../format";
import { monthNames, msg, t } from "../i18n";

const SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (n: number) => String(n).padStart(2, "0");
const iso = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const thisMonth = () => iso(new Date()).slice(0, 7);

/**
 * Month chooser: a calendar button opening a year of months. With `text`, the button shows it
 * with a down arrow instead.
 */
export function MonthPicker({
  value,
  onChange,
  label = t("Choose month"),
  text,
}: {
  value: string;
  onChange: (month: string) => void;
  label?: string;
  text?: string;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [year, setYear] = useState(Number(value.slice(0, 4)));
  const current = thisMonth();
  return (
    <>
      <button
        ref={button}
        className={text ? "month-button" : "icon-button"}
        aria-label={label}
        title={text ? undefined : label}
        aria-expanded={open}
        onClick={() => {
          setYear(Number(value.slice(0, 4)));
          setOpen(!open);
        }}
      >
        {text ? (
          <>
            {text} <ChevronDown size={18} aria-hidden />
          </>
        ) : (
          <CalendarDays size={15} />
        )}
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={label} className="month-grid">
        <div className="month-grid-head">
          <button className="icon-button" aria-label={t("Previous year")} onClick={() => setYear(year - 1)}>
            <ChevronLeft size={15} />
          </button>
          <strong>{year}</strong>
          <button className="icon-button" aria-label={t("Next year")} onClick={() => setYear(year + 1)}>
            <ChevronRight size={15} />
          </button>
        </div>
        <div className="month-grid-body">
          {(monthNames() ?? SHORT).map((name, i) => {
            const month = `${year}-${pad(i + 1)}`;
            return (
              <button
                key={month}
                className={`${month === value ? "selected" : ""}${month === current ? " today" : ""}`}
                aria-pressed={month === value}
                data-autofocus={month === value || undefined}
                onClick={() => {
                  onChange(month);
                  setOpen(false);
                }}
              >
                {name}
              </button>
            );
          })}
        </div>
        <button
          className="link-button"
          onClick={() => {
            onChange(current);
            setOpen(false);
          }}
        >
          {t("This month")}
        </button>
      </Popover>
    </>
  );
}

export type RangePreset =
  | "thisMonth"
  | "lastMonth"
  | "last3"
  | "last6"
  | "last12"
  | "ytd"
  | "lastYear"
  | "all"
  | "custom";
/** `from`/`to` are inclusive dates ("YYYY-MM-DD") or months ("YYYY-MM"); "" means unbounded. */
export type DateRange = { preset: RangePreset; from: string; to: string };
/** English labels, to show with `t(presetLabels[preset])`. */
export const presetLabels: Record<RangePreset, string> = {
  thisMonth: msg("This month"),
  lastMonth: msg("Last month"),
  last3: msg("Last 3 months"),
  last6: msg("Last 6 months"),
  last12: msg("Last 12 months"),
  ytd: msg("Year to date"),
  lastYear: msg("Last year"),
  all: msg("All dates"),
  custom: msg("Custom range"),
};

/** The dates a preset covers, at day or month granularity, relative to `today`. */
export function rangeFor(
  preset: Exclude<RangePreset, "custom">,
  granularity: "day" | "month" = "day",
  today = new Date(),
  earliest = "",
): DateRange {
  const y = today.getFullYear(),
    m = today.getMonth();
  const start = (months: number) => new Date(y, m - months, 1);
  const end = (months: number) => new Date(y, m - months + 1, 0);
  const [from, to] = {
    thisMonth: [start(0), end(0)],
    lastMonth: [start(1), end(1)],
    last3: [start(2), end(0)],
    last6: [start(5), end(0)],
    last12: [start(11), end(0)],
    ytd: [new Date(y, 0, 1), end(0)],
    lastYear: [new Date(y - 1, 0, 1), new Date(y - 1, 11, 31)],
    all: [null, end(0)],
  }[preset];
  const cut = (d: string) => (granularity === "month" ? d.slice(0, 7) : d);
  return {
    preset,
    from: from ? cut(iso(from)) : cut(earliest),
    to: preset === "all" && granularity === "day" ? "" : cut(iso(to!)),
  };
}

const describe = (r: DateRange, granularity: "day" | "month") => {
  if (r.preset !== "custom") return t(presetLabels[r.preset]);
  const show = (v: string) => (!v ? "…" : granularity === "month" ? monthName(v, "short") : formatDate(v));
  return `${show(r.from)} – ${show(r.to)}`;
};

/** A button showing the range, opening presets and a custom from/to. */
export function DateRangePicker({
  value,
  onChange,
  granularity = "day",
  earliest = "",
  label = t("Date range"),
}: {
  value: DateRange;
  onChange: (range: DateRange) => void;
  granularity?: "day" | "month";
  /** First date (or month) with data, used by "All dates". */
  earliest?: string;
  label?: string;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [from, setFrom] = useState(value.from);
  const [to, setTo] = useState(value.to);
  const type = granularity === "month" ? "month" : "date";
  return (
    <>
      <button
        ref={button}
        className="secondary range-button"
        aria-label={`${label}: ${describe(value, granularity)}`}
        aria-expanded={open}
        onClick={() => {
          setFrom(value.from);
          setTo(value.to);
          setOpen(!open);
        }}
      >
        <CalendarDays size={15} /> {describe(value, granularity)}
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={label} className="range-popover">
        <div className="range-presets" role="listbox" aria-label={t("Presets")}>
          {(Object.keys(presetLabels) as RangePreset[])
            .filter((p) => p !== "custom")
            .map((p) => (
              <button
                key={p}
                role="option"
                aria-selected={value.preset === p}
                className={value.preset === p ? "selected" : ""}
                data-autofocus={value.preset === p || undefined}
                onClick={() => {
                  onChange(rangeFor(p as Exclude<RangePreset, "custom">, granularity, new Date(), earliest));
                  setOpen(false);
                }}
              >
                {t(presetLabels[p])}
              </button>
            ))}
        </div>
        <form
          className="range-custom"
          onSubmit={(e) => {
            e.preventDefault();
            const [a, b] = from && to && from > to ? [to, from] : [from, to];
            onChange({ preset: "custom", from: a, to: b });
            setOpen(false);
          }}
        >
          <strong>{t("Custom range")}</strong>
          <label>
            {t("From")}
            {type === "date" ? (
              <DateInput label={t("From")} value={from} onChange={setFrom} />
            ) : (
              <input type={type} value={from} onChange={(e) => setFrom(e.target.value)} />
            )}
          </label>
          <label>
            {t("To")}
            {type === "date" ? (
              <DateInput label={t("To")} value={to} onChange={setTo} />
            ) : (
              <input type={type} value={to} onChange={(e) => setTo(e.target.value)} />
            )}
          </label>
          <button className="primary">{t("Apply")}</button>
        </form>
      </Popover>
    </>
  );
}
