import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { getDirection } from "../i18n";

/**
 * A row of tabs (or view filters); the arrow keys move between them. They stay on one line:
 * when they don't fit they scroll sideways, the cut edge fades, and the selected tab is kept in view.
 */
export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
  label,
  className = "",
}: {
  tabs: { id: T; label: ReactNode; count?: number }[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  className?: string;
}) {
  const list = useRef<HTMLDivElement>(null);
  const [fade, setFade] = useState<"start" | "end" | "both" | undefined>();
  const measure = () => {
    const el = list.current;
    if (!el) return;
    // Right to left, scrollLeft runs from 0 at the start towards negative values.
    const scrolled = Math.abs(el.scrollLeft);
    const start = scrolled > 1,
      end = scrolled + el.clientWidth < el.scrollWidth - 1;
    setFade(start && end ? "both" : start ? "start" : end ? "end" : undefined);
  };
  useLayoutEffect(() => {
    const el = list.current;
    if (!el) return;
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [tabs.length]);
  useLayoutEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-tab="${value}"]`)?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [value]);
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    const index = tabs.findIndex((t) => t.id === value);
    // The arrow pointing along the row moves on: the left one, right to left.
    const [forward, back] = getDirection() === "rtl" ? ["ArrowLeft", "ArrowRight"] : ["ArrowRight", "ArrowLeft"];
    const step = e.key === forward ? 1 : e.key === back ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const el = e.currentTarget;
    const next = tabs[(index + step + tabs.length) % tabs.length];
    onChange(next.id);
    requestAnimationFrame(() => el.querySelector<HTMLElement>(`[data-tab="${next.id}"]`)?.focus());
  };
  return (
    <div
      ref={list}
      className={`tabs ${className}`}
      role="tablist"
      aria-label={label}
      data-fade={fade}
      onKeyDown={key}
      onScroll={measure}
    >
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          data-tab={t.id}
          aria-selected={t.id === value}
          tabIndex={t.id === value ? 0 : -1}
          className={t.id === value ? "selected" : ""}
          onClick={() => onChange(t.id)}
        >
          {t.label}
          {t.count !== undefined && <small>{t.count}</small>}
        </button>
      ))}
    </div>
  );
}
