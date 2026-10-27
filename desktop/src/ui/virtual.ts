import { useLayoutEffect, useState, type RefObject } from "react";

/**
 * Which of `count` rows of a fixed height to render in a scrolling box: the ones in view and a
 * few either side. `before` and `after` are the heights the rows left out would take, for
 * spacers above and below, so the scrollbar still matches the whole list.
 */
export function useVirtualRows(scroller: RefObject<HTMLElement | null>, count: number, rowHeight: number, overscan = 10) {
  const [view, setView] = useState({ top: 0, height: 600 });
  const shown = count > 0;
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // Only a scroll that brings another row into view changes what renders.
    const update = () => {
      const top = Math.floor(el.scrollTop / rowHeight) * rowHeight;
      const height = el.clientHeight || 600;
      setView((was) => (was.top === top && was.height === height ? was : { top, height }));
    };
    update();
    el.addEventListener("scroll", update, { passive: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(el);
    return () => {
      el.removeEventListener("scroll", update);
      observer?.disconnect();
    };
    // The box appears once there are rows to show.
  }, [scroller, shown, rowHeight]);
  // A filtered or refreshed list can become shorter before the browser delivers its
  // scroll-clamping event. Keep rendering its last page instead of a blank spacer.
  const top = Math.min(view.top, Math.max(0, count * rowHeight - view.height));
  const start = Math.max(0, Math.floor(top / rowHeight) - overscan);
  const end = Math.min(count, Math.ceil((top + view.height) / rowHeight) + overscan);
  return {
    start,
    end,
    before: start * rowHeight,
    after: Math.max(0, count - end) * rowHeight,
  };
}
