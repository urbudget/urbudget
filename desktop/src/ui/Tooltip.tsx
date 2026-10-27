import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getDirection } from "../i18n";

/** How long the pointer rests before the first tooltip shows. */
const DELAY = 500;
/** After a tooltip hides, the next one shows at once for this long (moving along a toolbar). */
const WARM = 500;
const GAP = 6;
const MARGIN = 8;
const TITLED = '[title]:not([title=""])';
const NAV_KEYS = new Set(["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"]);

type Tip = { text: string; rect: DOMRect; x: number | null; side: "below" | "right" };

/**
 * Shows every `title` in the app in one styled tooltip, in place of the system's. While the
 * pointer is over an element its title is blanked (so the system tooltip stays away) and put
 * back when the pointer leaves. Moving focus with Tab or the arrow keys shows the same tooltip
 * and leaves the title alone, so screen readers still read it. `data-tooltip-side="right"`
 * puts it beside the element instead of below (on its left, right to left).
 */
export function TooltipLayer() {
  const [tip, setTip] = useState<Tip | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let anchor: Element | null = null;
    // The anchor's title; null once the page drops it.
    let text: string | null = null;
    // Shown for the pointer (its title is blanked) rather than for keyboard focus.
    let hover = false;
    let shown = false;
    let timer: number | undefined;
    let hiddenAt = -Infinity;
    let navKeyAt = -Infinity;
    let pointerX = 0;

    const show = () => {
      if (!anchor || !text) return;
      shown = true;
      setTip({
        text,
        rect: anchor.getBoundingClientRect(),
        x: hover ? pointerX : null,
        side: anchor.getAttribute("data-tooltip-side") === "right" ? "right" : "below",
      });
    };
    const hide = () => {
      clearTimeout(timer);
      if (shown) hiddenAt = Date.now();
      shown = false;
      setTip(null);
    };
    // React may change or drop a title while it is blanked: follow it.
    const sync = () => {
      if (!anchor || !hover) return;
      const now = anchor.getAttribute("title");
      if (now === "") return;
      text = now;
      if (now === null) return hide();
      anchor.setAttribute("title", "");
      if (shown) show();
    };
    const watcher = new MutationObserver(sync);
    const blank = () => {
      if (!anchor) return;
      hover = true;
      anchor.setAttribute("title", "");
      watcher.observe(anchor, { attributes: true, attributeFilter: ["title"] });
    };
    const end = () => {
      hide();
      if (anchor && hover) {
        sync();
        watcher.disconnect();
        if (text !== null && anchor.getAttribute("title") === "") anchor.setAttribute("title", text);
      }
      anchor = null;
      text = null;
      hover = false;
    };
    const start = (el: Element, viaPointer: boolean) => {
      if (el === anchor) {
        if (viaPointer && !hover) blank();
        return;
      }
      end();
      anchor = el;
      text = el.getAttribute("title");
      if (viaPointer) blank();
      if (Date.now() - hiddenAt < WARM) show();
      else timer = window.setTimeout(show, DELAY);
    };

    const over = (e: PointerEvent) => {
      if (e.pointerType === "touch") return;
      const target = e.target as Element;
      pointerX = e.clientX;
      let el = target.closest?.(TITLED) ?? null;
      // The anchor's own title is blank while hovered, so `closest` can skip past it.
      if (anchor?.contains(target) && (!el || !anchor.contains(el))) el = anchor;
      // No new tooltips while a button is held (dragging, selecting text).
      if (el && !e.buttons) start(el, true);
      else if (hover && el !== anchor) end();
    };
    const out = (e: PointerEvent) => {
      if (hover && anchor && !anchor.contains(e.relatedTarget as Node | null)) end();
    };
    const move = (e: PointerEvent) => {
      if (!shown) pointerX = e.clientX;
    };
    // A click or a key hides the tooltip; it comes back only after the pointer leaves.
    const dismiss = () => {
      if (anchor) hide();
    };
    const key = (e: KeyboardEvent) => {
      if (NAV_KEYS.has(e.key)) navKeyAt = Date.now();
      dismiss();
    };
    const focusIn = (e: FocusEvent) => {
      const target = e.target as Element;
      if (Date.now() - navKeyAt < 100 && target.matches?.(TITLED)) start(target, false);
    };
    const focusOut = (e: FocusEvent) => {
      if (!hover && e.target === anchor) end();
    };
    const scroll = () => {
      if (hover) dismiss();
      else if (shown) show();
    };

    addEventListener("pointerover", over, true);
    addEventListener("pointerout", out, true);
    addEventListener("pointermove", move, { capture: true, passive: true });
    addEventListener("pointerdown", dismiss, true);
    addEventListener("pointercancel", end, true);
    addEventListener("keydown", key, true);
    addEventListener("focusin", focusIn, true);
    addEventListener("focusout", focusOut, true);
    addEventListener("scroll", scroll, true);
    addEventListener("blur", end);
    return () => {
      end();
      removeEventListener("pointerover", over, true);
      removeEventListener("pointerout", out, true);
      removeEventListener("pointermove", move, true);
      removeEventListener("pointerdown", dismiss, true);
      removeEventListener("pointercancel", end, true);
      removeEventListener("keydown", key, true);
      removeEventListener("focusin", focusIn, true);
      removeEventListener("focusout", focusOut, true);
      removeEventListener("scroll", scroll, true);
      removeEventListener("blur", end);
    };
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !tip) return;
    const { rect, x } = tip;
    const w = el.offsetWidth,
      h = el.offsetHeight;
    let left: number, top: number;
    const rtl = getDirection() === "rtl";
    if (tip.side === "right" && (rtl ? rect.left - GAP - w >= MARGIN : rect.right + GAP + w <= innerWidth - MARGIN)) {
      left = rtl ? rect.left - GAP - w : rect.right + GAP;
      top = rect.top + rect.height / 2 - h / 2;
    } else {
      // A wide anchor (a row, a long name) puts the tooltip under the pointer, not its middle.
      const centre = x !== null && rect.width > 160 ? Math.min(Math.max(x, rect.left), rect.right) : rect.left + rect.width / 2;
      left = centre - w / 2;
      top = rect.bottom + GAP;
      if (top + h > innerHeight - MARGIN) top = rect.top - GAP - h;
    }
    el.style.left = `${Math.min(Math.max(MARGIN, left), innerWidth - w - MARGIN)}px`;
    el.style.top = `${Math.min(Math.max(MARGIN, top), innerHeight - h - MARGIN)}px`;
  }, [tip]);

  if (!tip) return null;
  return createPortal(
    <div ref={ref} className="tooltip" role="tooltip">
      {tip.text}
    </div>,
    document.body,
  );
}
