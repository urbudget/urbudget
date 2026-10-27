import { activeFormat } from "../format";
import { monthNames } from "../i18n";

/** Round tick values (in the data's units) covering [min, max], always including 0. */
export function niceTicks(min: number, max: number, count = 5): number[] {
  const lo = Math.min(0, min),
    hi = Math.max(0, max);
  if (lo === hi) return [0, 1];
  const raw = (hi - lo) / count;
  const power = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * power).find((s) => raw <= s) ?? 10 * power;
  const ticks: number[] = [];
  for (let v = Math.floor(lo / step) * step; v <= hi + step / 2; v += step) ticks.push(Math.round(v * 1e6) / 1e6);
  if (ticks[ticks.length - 1] < hi) ticks.push(ticks[ticks.length - 1] + step);
  return ticks;
}

/** Maps a domain onto a pixel range. */
export const linear = ([d0, d1]: [number, number], [r0, r1]: [number, number]) => (v: number) =>
  d1 === d0 ? r0 : r0 + ((v - d0) / (d1 - d0)) * (r1 - r0);

/** Axis label for an amount in cents: 0, 950, 1.5K, 12K, 1.2M. */
export function compactMoney(cents: number) {
  const v = cents / 100,
    abs = Math.abs(v),
    decimal = activeFormat().number.decimal;
  const fixed = (n: number, digits: number) => n.toFixed(digits).replace(/\.0+$/, "").replace(".", decimal);
  if (abs >= 1e6) return `${fixed(v / 1e6, abs >= 1e7 ? 0 : 1)}M`;
  if (abs >= 1e3) return `${fixed(v / 1e3, abs >= 1e4 ? 0 : 1)}K`;
  return fixed(v, 0);
}

/** "Oct" for each month; "Oct 26" when the year changes or on the first label. Arabic has no short month names. */
export function monthTick(month: string, previous?: string) {
  const name =
    monthNames()?.[Number(month.slice(5, 7)) - 1] ??
    new Date(`${month}-02T12:00:00`).toLocaleDateString("en-GB", { month: "short" });
  return !previous || previous.slice(0, 4) !== month.slice(0, 4) ? `${name} ${month.slice(2, 4)}` : name;
}

/**
 * Five steps of magnitude for a sequential colour: each positive value gets 1–5 by where it
 * falls among the others (quintiles), so a few large days don't wash everything else out.
 * Zero and below get 0 (no colour).
 */
export function heatLevels(values: number[]): (v: number) => number {
  const sorted = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (!sorted.length) return () => 0;
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  const cuts = [at(0.2), at(0.4), at(0.6), at(0.8)];
  return (v) => (v <= 0 ? 0 : 1 + cuts.filter((c) => v > c).length);
}

export type Box = { x: number; y: number; w: number; h: number };
/**
 * Squarified treemap: tiles with areas in proportion to their values, laid out in rows that keep
 * each tile as close to square as possible. Items must be positive, largest first.
 */
export function squarify<T extends { value: number }>(items: T[], box: Box): (T & Box)[] {
  const total = items.reduce((n, i) => n + i.value, 0);
  if (total <= 0 || box.w <= 0 || box.h <= 0) return [];
  const scale = (box.w * box.h) / total;
  const out: (T & Box)[] = [];
  let rest = { ...box };
  let row: T[] = [];
  const area = (i: T) => i.value * scale;
  const worst = (r: T[], side: number) => {
    const s = r.reduce((n, i) => n + area(i), 0);
    const max = Math.max(...r.map(area)),
      min = Math.min(...r.map(area));
    return Math.max((side * side * max) / (s * s), (s * s) / (side * side * min));
  };
  const place = (r: T[]) => {
    const s = r.reduce((n, i) => n + area(i), 0);
    if (rest.w >= rest.h) {
      const w = s / rest.h;
      let y = rest.y;
      for (const i of r) {
        const h = area(i) / w;
        out.push({ ...i, x: rest.x, y, w, h });
        y += h;
      }
      rest = { ...rest, x: rest.x + w, w: rest.w - w };
    } else {
      const h = s / rest.w;
      let x = rest.x;
      for (const i of r) {
        const w = area(i) / h;
        out.push({ ...i, x, y: rest.y, w, h });
        x += w;
      }
      rest = { ...rest, y: rest.y + h, h: rest.h - h };
    }
  };
  for (const item of items) {
    const side = Math.min(rest.w, rest.h);
    if (!row.length || worst([...row, item], side) <= worst(row, side)) row.push(item);
    else {
      place(row);
      row = [item];
    }
  }
  if (row.length) place(row);
  return out;
}
