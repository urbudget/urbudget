import { useCallback, useRef, useState, type PointerEvent } from "react";
import { getDirection, t } from "../i18n";

const MIN = 40;
const load = (id: string): Record<string, number> => {
  try {
    return JSON.parse(localStorage.getItem(`lb:columns:${id}`) ?? "{}") ?? {};
  } catch {
    return {};
  }
};
const save = (id: string, widths: Record<string, number>) => {
  try {
    localStorage.setItem(`lb:columns:${id}`, JSON.stringify(widths));
  } catch {
    // Widths are a convenience; without storage they reset next time.
  }
};

/**
 * Fixed column widths that don't change with the window or the content, which the user can
 * drag to resize (double-click a grip to reset). Remembered per table `id`. `set` holds only
 * the widths the user chose, for tables whose other columns size themselves.
 */
export function useColumns<K extends string>(
  id: string,
  defaults: Record<K, number>,
  floors: Partial<Record<K, number>> = {},
) {
  // Only widths the user changed are stored, so improved defaults still reach the others.
  const [changed, setChanged] = useState<Partial<Record<K, number>>>(() => load(id) as Partial<Record<K, number>>);
  const widths = Object.fromEntries(
    (Object.keys(defaults) as K[]).map((k) => [k, Math.max(floors[k] ?? MIN, changed[k] ?? defaults[k])]),
  ) as Record<K, number>;
  const latest = useRef(changed);
  latest.current = changed;
  // Defaults may be measured from the content, so a reset compares with the current ones.
  const current = useRef(defaults);
  current.current = defaults;
  const resize = useCallback(
    (key: K, width: number | null, done: boolean) => {
      const next = { ...latest.current };
      if (width === null || Math.round(width) === current.current[key]) delete next[key];
      else next[key] = Math.max(MIN, Math.round(width));
      setChanged(next);
      if (done) save(id, next as Record<string, number>);
    },
    [id],
  );
  const grip = (key: K, label: string) => (
    <ColumnGrip
      label={label}
      width={widths[key]}
      onResize={(w, done) => resize(key, w, done)}
      onReset={() => resize(key, null, true)}
    />
  );
  return { widths, set: changed, grip };
}

/** The drag handle on a column header's right edge (a pointer affordance, hidden from screen readers). */
export function ColumnGrip({
  label,
  width,
  onResize,
  onReset,
}: {
  label: string;
  width: number;
  onResize: (width: number, done: boolean) => void;
  onReset: () => void;
}) {
  const start = useRef<{ x: number; width: number } | null>(null);
  const move = (e: PointerEvent<HTMLSpanElement>, done: boolean) => {
    if (!start.current) return;
    // Right to left a column's grip is on its left edge: dragging left widens it.
    const moved = e.clientX - start.current.x;
    onResize(start.current.width + (getDirection() === "rtl" ? -moved : moved), done);
    if (done) start.current = null;
  };
  return (
    <span
      className="col-grip"
      aria-hidden
      data-column={label}
      title={t("Drag to resize · double-click to reset")}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onReset();
      }}
      onPointerDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        e.currentTarget.setPointerCapture(e.pointerId);
        // From the width it has on screen, which differs from `width` for a column that shares
        // spare room or has shrunk to fit the window.
        start.current = { x: e.clientX, width: e.currentTarget.parentElement?.getBoundingClientRect().width || width };
      }}
      onPointerMove={(e) => move(e, false)}
      onPointerUp={(e) => move(e, true)}
      onPointerCancel={() => (start.current = null)}
    />
  );
}

/**
 * A `<colgroup>` for a fixed-layout table (`.data-table.fixed`). `null` marks the one column
 * that takes any spare width, so the others keep their set widths; rows need a cell for it.
 */
export function Cols({ widths }: { widths: (number | null)[] }) {
  return (
    <colgroup>
      {widths.map((w, i) => (
        <col key={i} style={w === null ? undefined : { width: w }} />
      ))}
    </colgroup>
  );
}
/** The narrowest a fixed table may get before it scrolls sideways. */
export const tableWidth = (widths: (number | null)[]) => widths.reduce<number>((n, w) => n + (w ?? 0), 0);

let probe: HTMLSpanElement | undefined;
const measured = new Map<string, number>();
/**
 * How wide `text` is drawn in the app's font, for columns as wide as their content: `figures`
 * draws digits tabular, as amounts are. Measured by the page itself, which a canvas can't do for
 * tabular figures, once per text; without layout (tests), an estimate.
 */
export function textWidth(text: string, size: number, weight = 400, figures = false) {
  const key = `${size} ${weight} ${figures} ${text}`;
  const known = measured.get(key);
  if (known) return known;
  if (!probe) {
    probe = document.createElement("span");
    probe.setAttribute("aria-hidden", "true");
    probe.style.cssText = "position:fixed;top:0;left:-9999px;visibility:hidden;white-space:pre;font-family:var(--font)";
    document.body.append(probe);
  }
  probe.style.fontSize = `${size}px`;
  probe.style.fontWeight = String(weight);
  probe.style.fontVariantNumeric = figures ? "tabular-nums" : "normal";
  probe.textContent = text;
  const width = Math.ceil(probe.getBoundingClientRect().width);
  if (!width) return Math.ceil(text.length * size * 0.6);
  measured.set(key, width);
  return width;
}
