import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { CalendarDays, ChevronLeft, ChevronRight } from "lucide-react";
import { Popover } from "./Popover";
import { activeFormat, formatDate, monthName, parseDate, shiftDate } from "../format";
import { usePref } from "../prefs";
import { getDirection, msg, stripIsolates, t } from "../i18n";

const pad = (n: number) => String(n).padStart(2, "0");
const todayIso = () => new Date().toLocaleDateString("en-CA");
// Calendar column heads, Sunday first.
const WEEKDAYS = [msg("Su"), msg("Mo"), msg("Tu"), msg("We"), msg("Th"), msg("Fr"), msg("Sa")];
/** The locale's first day of the week (0 = Sunday), where the browser knows it. */
const localeFirstWeekday = (() => {
  try {
    const locale = new Intl.Locale(navigator.language) as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number };
      weekInfo?: { firstDay: number };
    };
    const day = (locale.getWeekInfo?.() ?? locale.weekInfo)?.firstDay;
    return day ? day % 7 : 0;
  } catch {
    return 0;
  }
})();
const placeholders: Record<string, string> = { iso: "YYYY-MM-DD", dmy: "DD/MM/YYYY", mdy: "MM/DD/YYYY", dmmmy: "D Mon YYYY" };

/** A month of days for picking a date; arrow keys move, Page Up/Down change month, Enter picks. */
/** The first day of the week (0 = Sunday), as set in Settings or by the system. */
export function useFirstWeekday() {
  const [weekStart] = usePref("weekStart");
  return weekStart === "auto" ? localeFirstWeekday : Number(weekStart);
}

export function Calendar({ value, onPick }: { value: string; onPick: (iso: string) => void }) {
  const today = todayIso();
  const firstWeekday = useFirstWeekday();
  const [focus, setFocus] = useState(value || today);
  const month = focus.slice(0, 7);
  const grid = useRef<HTMLDivElement>(null);
  useEffect(() => {
    grid.current?.querySelector<HTMLElement>(`[data-day="${focus}"]`)?.focus();
  }, [focus]);
  const days = useMemo(() => {
    const [y, m] = month.split("-").map(Number);
    const first = new Date(y, m - 1, 1, 12);
    const lead = (first.getDay() - firstWeekday + 7) % 7;
    return Array.from({ length: 42 }, (_, i) => {
      const d = new Date(y, m - 1, 1 - lead + i, 12);
      return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
    });
  }, [month, firstWeekday]);
  const keys = (e: KeyboardEvent) => {
    const move: Record<string, () => string> = {
      // Right to left the week runs leftwards, so the left arrow is the next day.
      ArrowLeft: () => shiftDate(focus, getDirection() === "rtl" ? 1 : -1),
      ArrowRight: () => shiftDate(focus, getDirection() === "rtl" ? -1 : 1),
      ArrowUp: () => shiftDate(focus, -7),
      ArrowDown: () => shiftDate(focus, 7),
      PageUp: () => shiftDate(focus, 0, -1),
      PageDown: () => shiftDate(focus, 0, 1),
      Home: () => shiftDate(focus, -((new Date(`${focus}T12:00`).getDay() - firstWeekday + 7) % 7)),
      End: () => shiftDate(focus, 6 - ((new Date(`${focus}T12:00`).getDay() - firstWeekday + 7) % 7)),
    };
    if (move[e.key]) {
      e.preventDefault();
      setFocus(move[e.key]());
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onPick(focus);
    }
  };
  return (
    <div className="calendar">
      <div className="calendar-head">
        <button type="button" className="icon-button" aria-label={t("Previous month")} onClick={() => setFocus(shiftDate(focus, 0, -1))}>
          <ChevronLeft size={15} />
        </button>
        <strong aria-live="polite">{monthName(month)}</strong>
        <button type="button" className="icon-button" aria-label={t("Next month")} onClick={() => setFocus(shiftDate(focus, 0, 1))}>
          <ChevronRight size={15} />
        </button>
      </div>
      <div className="calendar-grid" role="grid" aria-label={monthName(month)} ref={grid} onKeyDown={keys}>
        {Array.from({ length: 7 }, (_, i) => (
          <span key={i} className="calendar-weekday" aria-hidden>
            {t(WEEKDAYS[(i + firstWeekday) % 7])}
          </span>
        ))}
        {days.map((d) => (
          <button
            type="button"
            key={d}
            data-day={d}
            data-autofocus={d === focus || undefined}
            tabIndex={d === focus ? 0 : -1}
            aria-label={formatDate(d)}
            aria-pressed={d === value}
            className={`calendar-day${d.slice(0, 7) !== month ? " outside" : ""}${d === today ? " today" : ""}${d === value ? " selected" : ""}`}
            onClick={() => onPick(d)}
          >
            {Number(d.slice(8))}
          </button>
        ))}
      </div>
      <div className="calendar-foot">
        <button type="button" className="link-button" onClick={() => onPick(today)}>
          {t("Today")}
        </button>
        <span className="muted">{formatDate(focus)}</span>
      </div>
    </div>
  );
}

/**
 * A date field in the budget's date format. Type a date ("15/10", "15 Oct", "today"), use
 * ↑/↓ for a day either way, or open the calendar (the button, or Alt+↓). `value` is ISO.
 */
export function DateInput({
  value,
  onChange,
  label = t("Date"),
  autoFocus,
  className = "",
  required,
}: {
  value: string;
  onChange: (iso: string) => void;
  label?: string;
  autoFocus?: boolean;
  className?: string;
  required?: boolean;
}) {
  const box = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // The text being typed: the date without the marks that keep it left to right in Arabic.
  const shown = (iso: string) => stripIsolates(formatDate(iso));
  const [text, setText] = useState(value ? shown(value) : "");
  const [editing, setEditing] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!editing && !invalid) setText(value ? shown(value) : "");
  }, [value, editing, invalid]);
  // An import can open on a bank value that has not parsed yet. Keep that text available to
  // correct, but anchor the calendar and arrow shortcuts to a real date.
  const validValue = /^\d{4}-\d{2}-\d{2}$/.test(value) && parseDate(value, value) === value;
  const base = validValue ? value : todayIso();
  const pick = (iso: string) => {
    onChange(iso);
    setText(shown(iso));
    setInvalid(false);
    input.current?.setCustomValidity("");
    setOpen(false);
    input.current?.focus();
  };
  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.altKey && e.key === "ArrowDown") {
      e.preventDefault();
      setOpen(true);
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      pick(shiftDate(base, e.key === "ArrowUp" ? 1 : -1));
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      e.preventDefault();
      pick(shiftDate(base, 0, e.key === "PageUp" ? 1 : -1));
    }
  };
  return (
    <div className={`date-input${invalid ? " invalid" : ""} ${className}`} ref={box}>
      <input
        ref={input}
        aria-label={label}
        aria-invalid={invalid || undefined}
        required={required}
        autoFocus={autoFocus}
        inputMode="numeric"
        placeholder={placeholders[activeFormat().date]}
        value={text}
        title={t("Type a date (e.g. 15/10, 15 Oct, today), use ↑/↓ to change it, or Alt+↓ for the calendar")}
        onFocus={(e) => {
          setEditing(true);
          e.currentTarget.select();
        }}
        onChange={(e) => {
          setText(e.target.value);
          // Each complete date counts straight away, so Enter in a form saves what's shown.
          const iso = parseDate(e.target.value, base);
          const bad = !!e.target.value.trim() && !iso;
          setInvalid(bad);
          e.currentTarget.setCustomValidity(bad ? t("Enter a valid date.") : "");
          if (iso) onChange(iso);
          else if (!e.target.value.trim()) onChange("");
        }}
        onBlur={() => {
          setEditing(false);
          if (!invalid) setText(value ? shown(value) : "");
        }}
        onKeyDown={keys}
      />
      <button
        type="button"
        className="date-input-button"
        aria-label={t("Choose {field}", { field: label.toLowerCase() })}
        aria-expanded={open}
        tabIndex={-1}
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          // Return to this opener on dismiss, not a previously focused field. Focusing the button
          // also avoids opening a text keyboard.
          e.currentTarget.focus();
          setOpen(!open);
        }}
      >
        <CalendarDays size={14} />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchor={box} label={t("Choose {field}", { field: label.toLowerCase() })} className="calendar-popover">
        <Calendar value={validValue ? value : ""} onPick={pick} />
      </Popover>
    </div>
  );
}
