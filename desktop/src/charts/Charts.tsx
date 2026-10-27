import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { compactMoney, heatLevels, linear, monthTick, niceTicks, squarify } from "./scale";
import { activeFormat, monthName } from "../format";
import { list, ltr, monthNames, rtlRun, t, weekdayNames } from "../i18n";

// Hand-written SVG charts. Marks follow the data-viz method: bars at most 24px wide
// with a 4px rounded data end, 2px surface gaps between stacked segments and slices, 2px lines,
// end dots of r=4 with a 2px surface ring, hairline solid grid, text in text colours,
// and a hover/focus tooltip on every mark. Colours are CSS custom properties.

export type Series = { key: string; label: string; color: string; values: number[] };
type Format = (cents: number) => string;
/** An x-axis label for a key (a month by default), or null to leave that position blank. */
type Tick = (key: string, i: number) => string | null;
const PAD = { top: 26, right: 16, bottom: 26, left: 52 };
const BAR = 24,
  GAP = 2,
  RADIUS = 4;
const monthTitle = (m: string) => monthName(m);

function useWidth(fallback = 720) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(fallback);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setWidth(el.clientWidth || fallback);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [fallback]);
  return [ref, width] as const;
}

/** A bar with a rounded data end and a square base. `up`: the data end is at the top. */
function barPath(x: number, y: number, w: number, h: number, up: boolean, rounded: boolean) {
  if (h <= 0) return "";
  const r = rounded ? Math.min(RADIUS, w / 2, h) : 0;
  return up
    ? `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`
    : `M${x},${y}V${y + h - r}Q${x},${y + h} ${x + r},${y + h}H${x + w - r}Q${x + w},${y + h} ${x + w},${y + h - r}V${y}Z`;
}

export type TooltipRow = { label: string; value: string; color?: string; line?: boolean };
function Tooltip({ x, y, width, title, rows }: { x: number; y: number; width: number; title: string; rows: TooltipRow[] }) {
  const flip = x > width - 200;
  return (
    <div className="chart-tooltip" role="status" style={{ left: flip ? undefined : x + 12, right: flip ? width - x + 12 : undefined, top: Math.max(0, y) }}>
      <div className="chart-tooltip-title">{title}</div>
      {rows.map((r) => (
        <div key={r.label} className="chart-tooltip-row">
          {r.color && <i className={r.line ? "key-line" : "key-box"} style={{ background: r.color }} />}
          {r.value && <strong>{r.value}</strong>}
          <span>{r.label}</span>
        </div>
      ))}
    </div>
  );
}

/** Legend keys mirror the marks: boxes for bars, short lines for lines (dashed for dashed lines). */
export function Legend({ items }: { items: { label: string; color: string; line?: boolean; dashed?: boolean }[] }) {
  return (
    <ul className="chart-legend">
      {items.map((i) => (
        <li key={i.label}>
          <i
            className={i.line ? `key-line${i.dashed ? " dashed" : ""}` : "key-box"}
            style={i.dashed ? { color: i.color } : { background: i.color }}
          />
          {i.label}
        </li>
      ))}
    </ul>
  );
}

/** X labels that never crowd: each needs ~44px from the last one drawn. */
function xLabels(keys: string[], x: (i: number) => number, tick?: Tick) {
  const out: { key: string; x: number; text: string }[] = [];
  let last = -Infinity;
  keys.forEach((k, i) => {
    const text = tick ? tick(k, i) : monthTick(k, out[out.length - 1]?.key);
    if (text === null || x(i) - last < 44) return;
    out.push({ key: k, x: x(i), text });
    last = x(i);
  });
  return out;
}

function Axes({
  ticks,
  y,
  width,
  keys,
  x,
  tick,
  axisFormat = compactMoney,
}: {
  ticks: number[];
  y: (v: number) => number;
  width: number;
  keys: string[];
  x: (i: number) => number;
  tick?: Tick;
  axisFormat?: (v: number) => string;
}) {
  return (
    <g className="chart-axes" aria-hidden>
      {axisFormat === compactMoney && activeFormat().number.symbol !== "none" && (
        <text x={PAD.left - 8} y={10} textAnchor="end">{activeFormat().currency}</text>
      )}
      {ticks.map((t) => (
        <g key={t}>
          <line x1={PAD.left} x2={width - PAD.right} y1={y(t)} y2={y(t)} className={t === 0 ? "baseline" : "grid"} />
          <text x={PAD.left - 8} y={y(t)} dy="0.32em" textAnchor="end">
            {axisFormat(t)}
          </text>
        </g>
      ))}
      {xLabels(keys, x, tick).map((l) => (
        <text key={l.key} x={l.x} y={y(ticks[0]) + 18} textAnchor="middle">
          {l.text}
        </text>
      ))}
    </g>
  );
}

/**
 * Columns by month (or any keys): one series, stacked series (positives stack up, negatives
 * down) or `grouped` series side by side, with an optional overlay line (same units, same axis)
 * and an optional average rule.
 */
export function ColumnChart({
  months,
  series,
  line,
  average,
  grouped = false,
  height = 240,
  label,
  format,
  tick,
  title = monthTitle,
  footer,
}: {
  months: string[];
  series: Series[];
  line?: { label: string; color: string; values: (number | null)[] };
  average?: { label: string; value: number };
  grouped?: boolean;
  height?: number;
  label: string;
  format: Format;
  tick?: Tick;
  title?: (key: string) => string;
  footer?: ReactNode;
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);
  const plot = { w: Math.max(10, width - PAD.left - PAD.right), h: height - PAD.top - PAD.bottom };
  const ups = months.map((_, i) =>
    grouped ? Math.max(0, ...series.map((s) => s.values[i] ?? 0)) : series.reduce((n, s) => n + Math.max(0, s.values[i] ?? 0), 0),
  );
  const downs = months.map((_, i) =>
    grouped ? Math.min(0, ...series.map((s) => s.values[i] ?? 0)) : series.reduce((n, s) => n + Math.min(0, s.values[i] ?? 0), 0),
  );
  const lineValues = (line?.values ?? []).filter((v): v is number => v !== null);
  const ticks = niceTicks(Math.min(0, ...downs, ...lineValues), Math.max(0, ...ups, ...lineValues, average?.value ?? 0));
  const y = linear([ticks[0], ticks[ticks.length - 1]], [PAD.top + plot.h, PAD.top]);
  const band = plot.w / Math.max(1, months.length);
  const x = (i: number) => PAD.left + band * (i + 0.5);
  const barWidth = Math.min(BAR, Math.max(2, band * 0.62));
  const n = Math.max(1, series.length);
  const groupWidth = Math.min(band * 0.8, n * BAR + (n - 1) * GAP);
  const each = Math.max(2, (groupWidth - (n - 1) * GAP) / n);
  const columns = months.map((m, i) => {
    if (grouped) {
      const parts = series.flatMap((s, k) => {
        const v = s.values[i] ?? 0;
        if (!v) return [];
        const left = x(i) - groupWidth / 2 + k * (each + GAP);
        const size = Math.abs(y(v) - y(0));
        const path = v > 0 ? barPath(left, y(v), each, size, true, true) : barPath(left, y(0), each, size, false, true);
        return [{ key: s.key, color: s.color, path }];
      });
      return { month: m, parts };
    }
    let top = y(0),
      bottom = y(0);
    const stacked = series.filter((s) => (s.values[i] ?? 0) !== 0);
    const lastUp = [...stacked].reverse().find((s) => s.values[i] > 0);
    const lastDown = [...stacked].reverse().find((s) => s.values[i] < 0);
    const parts = stacked.map((s) => {
      const v = s.values[i];
      const size = Math.abs(y(v) - y(0));
      if (v > 0) {
        const yTop = top - size;
        const path = barPath(x(i) - barWidth / 2, yTop, barWidth, size - (top !== y(0) ? GAP : 0), true, s === lastUp);
        top = yTop;
        return { key: s.key, color: s.color, path };
      }
      const path = barPath(x(i) - barWidth / 2, bottom + (bottom !== y(0) ? GAP : 0), barWidth, size - (bottom !== y(0) ? GAP : 0), false, s === lastDown);
      bottom += size;
      return { key: s.key, color: s.color, path };
    });
    return { month: m, parts };
  });
  const linePath =
    line &&
    line.values
      .map((v, i) => (v === null ? null : `${x(i)},${y(v)}`))
      .reduce((d, p, i, all) => (p === null ? d : `${d}${i === 0 || all[i - 1] === null ? "M" : "L"}${p}`), "");
  const lastLine = line ? line.values.reduce<number>((last, v, i) => (v === null ? last : i), -1) : -1;
  const rows = (i: number): TooltipRow[] => [
    ...series.filter((s) => s.values[i]).map((s) => ({ label: s.label, value: format(s.values[i]), color: s.color })),
    ...(series.length > 1 && !grouped ? [{ label: t("Total"), value: format(series.reduce((n, s) => n + (s.values[i] ?? 0), 0)) }] : []),
    ...(line && line.values[i] !== null ? [{ label: line.label, value: format(line.values[i]!), color: line.color, line: true }] : []),
  ];
  return (
    <div className="chart" ref={ref}>
      <svg width={width} height={height} role="img" aria-label={label}>
        <Axes ticks={ticks} y={y} width={width} keys={months} x={x} tick={tick} />
        {hover !== null && (
          <rect className="chart-hover" x={x(hover) - band / 2} y={PAD.top} width={band} height={plot.h} />
        )}
        {columns.map((c) => c.parts.map((p) => <path key={`${c.month}-${p.key}`} d={p.path} fill={p.color} />))}
        {average && (
          <g className="chart-average">
            <line x1={PAD.left} x2={width - PAD.right} y1={y(average.value)} y2={y(average.value)} />
            <text x={width - PAD.right} y={y(average.value) - 5} textAnchor="end">
              {rtlRun(`${average.label} ${format(average.value)}`)}
            </text>
          </g>
        )}
        {linePath && (
          <g>
            <path d={linePath} fill="none" stroke={line!.color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
            {lastLine >= 0 && (
              <circle cx={x(lastLine)} cy={y(line!.values[lastLine]!)} r={4} fill={line!.color} className="chart-dot" />
            )}
          </g>
        )}
        {months.map((m, i) => (
          <rect
            key={m}
            className="chart-hit"
            x={x(i) - band / 2}
            y={PAD.top}
            width={band}
            height={plot.h}
            tabIndex={0}
            aria-label={t("{label}: {value}", { label: title(m), value: list(rows(i).map((r) => `${r.label} ${r.value}`)) })}
            onPointerEnter={() => setHover(i)}
            onPointerLeave={() => setHover(null)}
            onFocus={() => setHover(i)}
            onBlur={() => setHover(null)}
          />
        ))}
      </svg>
      {hover !== null && <Tooltip x={x(hover)} y={PAD.top} width={width} title={title(months[hover])} rows={rows(hover)} />}
      {footer}
    </div>
  );
}

export type LineSeries = { key: string; label: string; color: string; values: (number | null)[]; dashed?: boolean };

/**
 * Lines over months (or days): 2px lines on one axis that includes zero, a crosshair tooltip
 * listing every series, and end labels when there are few enough lines to label directly.
 * Focus the chart and use ←/→ to step through the points.
 */
export function LinesChart({
  keys,
  series,
  label,
  format,
  axisFormat = compactMoney,
  height = 240,
  tick,
  title = monthTitle,
  wash = series.length === 1,
}: {
  keys: string[];
  series: LineSeries[];
  label: string;
  format: (v: number) => string;
  axisFormat?: (v: number) => string;
  height?: number;
  tick?: Tick;
  title?: (key: string) => string;
  /** A 10% fill under a single line. */
  wash?: boolean;
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);
  const plot = { w: Math.max(10, width - PAD.left - PAD.right), h: height - PAD.top - PAD.bottom };
  const known = series.flatMap((s) => s.values.filter((v): v is number => v !== null));
  const ticks = niceTicks(Math.min(0, ...known), Math.max(1, ...known));
  const y = linear([ticks[0], ticks[ticks.length - 1]], [PAD.top + plot.h, PAD.top]);
  const step = plot.w / Math.max(1, keys.length - 1);
  const x = (i: number) => PAD.left + (keys.length === 1 ? plot.w / 2 : step * i);
  const lastOf = (s: LineSeries) => s.values.reduce<number>((n, v, i) => (v === null ? n : i), -1);
  // End labels for up to four lines, nudged apart so they never overlap.
  const ends = series
    .map((s) => ({ s, i: lastOf(s) }))
    .filter((e) => e.i >= 0)
    .map((e) => ({ ...e, y: y(e.s.values[e.i]!) - 10 }))
    .sort((a, b) => a.y - b.y);
  for (let k = 1; k < ends.length; k++) ends[k].y = Math.max(ends[k].y, ends[k - 1].y + 14);
  const rows = (i: number): TooltipRow[] =>
    series
      .filter((s) => s.values[i] !== null && s.values[i] !== undefined)
      .map((s) => ({ label: s.label, value: format(s.values[i]!), color: s.color, line: true }));
  const keyStep = (e: KeyboardEvent) => {
    const last = keys.length - 1;
    const moves: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1 };
    if (e.key in moves) setHover((h) => Math.min(last, Math.max(0, (h ?? last) + moves[e.key])));
    else if (e.key === "Home") setHover(0);
    else if (e.key === "End") setHover(last);
    else return;
    e.preventDefault();
  };
  return (
    <div className="chart" ref={ref}>
      <svg
        width={width}
        height={height}
        role="img"
        aria-label={label}
        tabIndex={0}
        className="chart-focus"
        onKeyDown={keyStep}
        onFocus={() => setHover((h) => h ?? keys.length - 1)}
        onBlur={() => setHover(null)}
        onPointerMove={(e) => {
          const bounds = e.currentTarget.getBoundingClientRect();
          const i = Math.round((e.clientX - bounds.left - PAD.left) / Math.max(step, 1));
          setHover(Math.min(keys.length - 1, Math.max(0, i)));
        }}
        onPointerLeave={() => setHover(null)}
      >
        <Axes ticks={ticks} y={y} width={width} keys={keys} x={x} tick={tick} axisFormat={axisFormat} />
        {series.map((s) => {
          const segments: { i: number; v: number }[][] = [];
          s.values.forEach((v, i) => {
            if (v === null) return;
            if (i === 0 || s.values[i - 1] === null) segments.push([]);
            segments[segments.length - 1].push({ i, v });
          });
          return segments.map((seg, k) => {
            const d = seg.map((p, j) => `${j ? "L" : "M"}${x(p.i)},${y(p.v)}`).join("");
            return (
              <g key={`${s.key}-${k}`}>
                {wash && <path d={`${d}L${x(seg[seg.length - 1].i)},${y(0)}L${x(seg[0].i)},${y(0)}Z`} fill={s.color} opacity={0.1} />}
                <path
                  d={d}
                  fill="none"
                  stroke={s.color}
                  strokeWidth={2}
                  strokeLinejoin="round"
                  strokeLinecap="round"
                  strokeDasharray={s.dashed ? "6 4" : undefined}
                />
              </g>
            );
          });
        })}
        {hover !== null && <line className="chart-crosshair" x1={x(hover)} x2={x(hover)} y1={PAD.top} y2={PAD.top + plot.h} />}
        {hover !== null &&
          series.map((s) =>
            s.values[hover] === null || s.values[hover] === undefined ? null : (
              <circle key={s.key} cx={x(hover)} cy={y(s.values[hover]!)} r={4} fill={s.color} className="chart-dot" />
            ),
          )}
        {ends.map((e) => (
          <g key={e.s.key}>
            <circle cx={x(e.i)} cy={y(e.s.values[e.i]!)} r={4} fill={e.s.color} className="chart-dot" />
            {series.length <= 4 && (
              <text className="chart-end-label" x={x(e.i) - 8} y={e.y} textAnchor="end">
                {format(e.s.values[e.i]!)}
              </text>
            )}
          </g>
        ))}
      </svg>
      {hover !== null && rows(hover).length > 0 && (
        <Tooltip x={x(hover)} y={PAD.top} width={width} title={title(keys[hover])} rows={rows(hover)} />
      )}
    </div>
  );
}

/** One series over months: a 2px line over a 10% wash, crosshair tooltip, end label. */
export function LineChart({
  months,
  values,
  color,
  label,
  seriesLabel,
  format,
  axisFormat = (v) => String(v),
  height = 220,
}: {
  months: string[];
  values: (number | null)[];
  color: string;
  label: string;
  seriesLabel: string;
  format: (v: number) => string;
  axisFormat?: (v: number) => string;
  height?: number;
}) {
  return (
    <LinesChart
      keys={months}
      series={[{ key: "line", label: seriesLabel, color, values }]}
      label={label}
      format={format}
      axisFormat={axisFormat}
      height={height}
    />
  );
}

export type Slice = { key: string; label: string; value: number; color: string };
/**
 * Parts of a whole as a ring, largest first, with a 2px surface gap between slices and the total
 * in the middle. Hover or focus a slice for its amount and share; `onSelect` makes slices buttons.
 */
export function Donut({
  slices,
  label,
  format,
  center,
  onSelect,
  size = 220,
}: {
  slices: Slice[];
  label: string;
  format: Format;
  center: { value: string; label: string };
  onSelect?: (key: string) => void;
  size?: number;
}) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = slices.filter((s) => s.value > 0);
  const total = shown.reduce((n, s) => n + s.value, 0);
  const c = size / 2,
    outer = c - 2,
    inner = outer * 0.62;
  const point = (r: number, a: number) => `${c + r * Math.sin(a)},${c - r * Math.cos(a)}`;
  let angle = 0;
  const arcs = shown.map((s) => {
    const a0 = angle,
      a1 = angle + (s.value / total) * Math.PI * 2;
    angle = a1;
    const end = Math.min(a1, a0 + Math.PI * 2 - 1e-4);
    const large = end - a0 > Math.PI ? 1 : 0;
    const path = `M${point(outer, a0)}A${outer},${outer} 0 ${large} 1 ${point(outer, end)}L${point(inner, end)}A${inner},${inner} 0 ${large} 0 ${point(inner, a0)}Z`;
    const mid = (a0 + a1) / 2;
    return { s, path, tip: { x: c + ((outer + inner) / 2) * Math.sin(mid), y: c - ((outer + inner) / 2) * Math.cos(mid) } };
  });
  const share = (v: number) => `${((v / total) * 100).toFixed(1)}%`;
  return (
    <div className="donut" style={{ width: size, height: size }}>
      <svg width={size} height={size} role="group" aria-label={label}>
        {arcs.map((a, i) => (
          <path
            key={a.s.key}
            d={a.path}
            fill={a.s.color}
            className={`donut-slice${arcs.length === 1 ? " whole" : ""}${hover !== null && hover !== i ? " dim" : ""}${onSelect ? " selectable" : ""}`}
            tabIndex={0}
            role={onSelect ? "button" : "img"}
            aria-label={t("{label}: {amount}, {share}", { label: a.s.label, amount: format(a.s.value), share: share(a.s.value) })}
            onPointerEnter={() => setHover(i)}
            onPointerLeave={() => setHover(null)}
            onFocus={() => setHover(i)}
            onBlur={() => setHover(null)}
            onClick={() => onSelect?.(a.s.key)}
            onKeyDown={(e) => {
              if (onSelect && (e.key === "Enter" || e.key === " ")) {
                e.preventDefault();
                onSelect(a.s.key);
              }
            }}
          />
        ))}
        {!arcs.length && <circle cx={c} cy={c} r={(outer + inner) / 2} fill="none" className="donut-empty" strokeWidth={outer - inner} />}
      </svg>
      <div className="donut-center" aria-hidden>
        <strong>{center.value}</strong>
        <span>{center.label}</span>
      </div>
      {hover !== null && arcs[hover] && (
        <Tooltip
          x={arcs[hover].tip.x}
          y={arcs[hover].tip.y - 20}
          width={size + 200}
          title={arcs[hover].s.label}
          rows={[{ label: share(arcs[hover].s.value), value: format(arcs[hover].s.value), color: arcs[hover].s.color }]}
        />
      )}
    </div>
  );
}

const WEEKDAY = (d: number) =>
  weekdayNames()?.[d] ?? new Date(2026, 1, 1 + d, 12).toLocaleDateString("en-GB", { weekday: "short" });
const addDays = (iso: string, n: number) => {
  const d = new Date(`${iso}T12:00:00`);
  d.setDate(d.getDate() + n);
  return d.toLocaleDateString("en-CA");
};

/**
 * Days as a grid of weeks (columns) by weekday (rows), shaded in five steps of one hue by how
 * much was spent; days with nothing spent stay empty. Hover a day for its date and amount.
 */
export function HeatCalendar({
  from,
  to,
  values,
  format,
  formatDay,
  weekStart,
  label,
  onSelect,
}: {
  from: string;
  to: string;
  values: Record<string, number>;
  format: Format;
  formatDay: (iso: string) => string;
  weekStart: number;
  label: string;
  /** Clicking a day with spending. */
  onSelect?: (day: string) => void;
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<{ day: string; x: number; y: number } | null>(null);
  const lead = (new Date(`${from}T12:00:00`).getDay() - weekStart + 7) % 7;
  const days: string[] = [];
  for (let d = from; d <= to && days.length < 3700; d = addDays(d, 1)) days.push(d);
  const weeks = Math.ceil((days.length + lead) / 7);
  // Arabic weekday names are full words, so their column is wider.
  const left = weekdayNames() ? 50 : 30,
    top = 18;
  const cell = Math.max(9, Math.min(18, Math.floor((width - left) / Math.max(1, weeks)) - 3));
  const pitch = cell + 3;
  const level = heatLevels(days.map((d) => values[d] ?? 0));
  const at = (i: number) => ({ x: left + Math.floor((i + lead) / 7) * pitch, y: top + ((i + lead) % 7) * pitch });
  const months = days.flatMap((d, i) => (d.endsWith("-01") || i === 0 ? [{ d, x: at(i).x }] : []));
  return (
    <div className="chart heat-calendar" ref={ref}>
      <div className="heat-scroll">
        <svg width={left + weeks * pitch} height={top + 7 * pitch} role="img" aria-label={label}>
          <g className="chart-axes" aria-hidden>
            {months.map((m, k) =>
              k && m.x - months[k - 1].x < 28 ? null : (
                <text key={m.d} x={m.x} y={11}>
                  {monthNames()?.[Number(m.d.slice(5, 7)) - 1] ??
                    new Date(`${m.d}T12:00:00`).toLocaleDateString("en-GB", { month: "short" })}
                </text>
              ),
            )}
            {[1, 3, 5].map((r) => (
              <text key={r} x={0} y={top + r * pitch + cell / 2} dy="0.32em">
                {WEEKDAY((r + weekStart) % 7)}
              </text>
            ))}
          </g>
          {days.map((d, i) => {
            const p = at(i);
            return (
              <rect
                key={d}
                x={p.x}
                y={p.y}
                width={cell}
                height={cell}
                rx={2}
                className={`heat-cell level-${level(values[d] ?? 0)}${hover?.day === d ? " active" : ""}${onSelect && (values[d] ?? 0) > 0 ? " selectable" : ""}`}
                onPointerEnter={() => setHover({ day: d, x: p.x + cell, y: p.y })}
                onPointerLeave={() => setHover(null)}
                onClick={() => onSelect && (values[d] ?? 0) > 0 && onSelect(d)}
              />
            );
          })}
        </svg>
      </div>
      {hover && (
        <Tooltip
          x={Math.min(hover.x, width)}
          y={hover.y - 8}
          width={width}
          title={formatDay(hover.day)}
          rows={[
            { label: (values[hover.day] ?? 0) > 0 ? t("spent") : t("nothing spent"), value: format(values[hover.day] ?? 0) },
            ...(onSelect && (values[hover.day] ?? 0) > 0 ? [{ label: t("Click to list the transactions"), value: "" }] : []),
          ]}
        />
      )}
      <div className="heat-legend" aria-hidden>
        <i className="heat-cell level-0" /> {t("No spending")}
        <span className="heat-scale">
          {t("Less")} {[1, 2, 3, 4, 5].map((l) => <i key={l} className={`heat-cell level-${l}`} />)} {t("More")}
        </span>
      </div>
    </div>
  );
}

/**
 * Tiles with areas in proportion to their values (largest top-left), shaded by magnitude in one
 * hue; labels show where a tile has room. Each tile is a button when `onSelect` is given.
 */
export function Treemap({
  items,
  format,
  label,
  onSelect,
  height = 300,
}: {
  /** `detail` and `more` are extra lines for the tooltip (and the accessible name). */
  items: { key: string; label: string; value: number; detail?: string; more?: string }[];
  format: Format;
  label: string;
  onSelect?: (key: string) => void;
  height?: number;
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<{ key: string; x: number; y: number } | null>(null);
  const shown = items.filter((i) => i.value > 0).sort((a, b) => b.value - a.value);
  const level = heatLevels(shown.map((i) => i.value));
  const tiles = squarify(shown, { x: 0, y: 0, w: width, h: height });
  const tip = hover && tiles.find((t) => t.key === hover.key);
  // The tooltip follows the pointer (or sits on a focused tile) inside the map.
  const place = (key: string, x: number, y: number) => setHover({ key, x, y });
  return (
    <div className="treemap" ref={ref} style={{ height }}>
      <div role="list" aria-label={label}>
        {tiles.map((tile) => {
          const roomy = tile.w > 76 && tile.h > 40,
            labelled = tile.w > 44 && tile.h > 22;
          const text = list(
            [t("{label}: {value}", { label: tile.label, value: format(tile.value) }), tile.detail, tile.more].filter(
              (s): s is string => !!s,
            ),
          );
          return (
            <button
              key={tile.key}
              role="listitem"
              className={`tm-tile level-${level(tile.value)}${hover?.key === tile.key ? " active" : ""}`}
              style={{ left: tile.x + 1, top: tile.y + 1, width: Math.max(0, tile.w - 2), height: Math.max(0, tile.h - 2) }}
              aria-label={text}
              tabIndex={onSelect ? 0 : -1}
              onClick={() => {
                setHover(null);
                onSelect?.(tile.key);
              }}
              onPointerMove={(e) => {
                const box = ref.current!.getBoundingClientRect();
                place(tile.key, e.clientX - box.left, e.clientY - box.top);
              }}
              onPointerLeave={() => setHover(null)}
              onFocus={() => place(tile.key, tile.x + tile.w / 2, tile.y + tile.h / 2)}
              onBlur={() => setHover(null)}
            >
              {labelled && <span className="tm-label">{tile.label}</span>}
              {roomy && <span className="tm-value">{format(tile.value)}</span>}
            </button>
          );
        })}
      </div>
      {tip && hover && (
        <Tooltip
          x={hover.x}
          y={Math.min(hover.y + 14, height - 70)}
          width={width}
          title={tip.label}
          rows={[
            { label: tip.detail ?? "", value: format(tip.value) },
            ...(tip.more ? [{ label: tip.more, value: "" }] : []),
            ...(onSelect ? [{ label: t("Click to list the transactions"), value: "" }] : []),
          ]}
        />
      )}
    </div>
  );
}

/** Changes either side of a centre line: rows with one bar each, left for less, right for more. */
export function DivergingList({
  rows,
  format,
  label,
  colors,
}: {
  rows: { key: string; label: string; value: number; detail?: string }[];
  format: Format;
  label: string;
  colors: { more: string; less: string };
}) {
  const max = Math.max(1, ...rows.map((r) => Math.abs(r.value)));
  return (
    <ul className="diverging-list" aria-label={label}>
      {rows.map((r) => (
        <li key={r.key}>
          <span className="bar-list-label" title={r.label}>
            {r.label}
          </span>
          <span className="diverging-track" aria-hidden>
            <span className="half less">
              {r.value < 0 && <i style={{ width: `${(-r.value / max) * 100}%`, background: colors.less }} />}
            </span>
            <span className="half more">
              {r.value > 0 && <i style={{ width: `${(r.value / max) * 100}%`, background: colors.more }} />}
            </span>
          </span>
          <strong className="num">{ltr(`${r.value > 0 ? "+" : ""}${format(r.value)}`)}</strong>
          <span className="num muted">{r.detail}</span>
        </li>
      ))}
    </ul>
  );
}

/** Ranked horizontal bars as rows; each row can be a button and can carry its own colour key. */
export function BarList({
  rows,
  color,
  format,
  label,
}: {
  rows: { key: string; label: string; value: number; detail?: string; sub?: string; color?: string; onSelect?: () => void; hint?: string }[];
  color: string;
  format: Format;
  label: string;
}) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <ul className="bar-list" aria-label={label}>
      {rows.map((r) => {
        const content = (
          <>
            <span className="bar-list-label">
              {r.color && <i className="key-box" style={{ background: r.color }} />}
              <span>
                {r.label}
                {r.sub && <small className="muted">{r.sub}</small>}
              </span>
            </span>
            <span className="bar-list-track" aria-hidden>
              <i style={{ width: `${(Math.max(0, r.value) / max) * 100}%`, background: r.color ?? color }} />
            </span>
            <strong className="num">{format(r.value)}</strong>
            <span className="num muted">{r.detail}</span>
          </>
        );
        return (
          <li key={r.key}>
            {r.onSelect ? (
              <button onClick={r.onSelect} title={r.hint}>
                {content}
              </button>
            ) : (
              <div>{content}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
