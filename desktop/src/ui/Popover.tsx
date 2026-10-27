import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { getDirection } from "../i18n";

/** An element to attach to, or a point (for right-click menus). */
export type Anchor = RefObject<HTMLElement | null> | { x: number; y: number };
export type Placement = "bottom-start" | "bottom-end" | "top-start" | "right-start";
const isRef = (a: Anchor): a is RefObject<HTMLElement | null> => "current" in a;
/** Where a menu opened from `rect` starts: its left edge, or its right edge right to left. */
export const startEdge = (rect: DOMRect) => (getDirection() === "rtl" ? rect.right : rect.left);

/**
 * A floating panel attached to an anchor. It stays inside the window, flips above the
 * anchor when there is no room below, and closes on Escape or a click outside. A menu also
 * closes on a second press of its button, and when the window loses focus.
 */
export function Popover({
  open,
  onClose,
  anchor,
  placement = "bottom-start",
  matchWidth = false,
  focus = true,
  className = "",
  role = "dialog",
  modal = role === "menu",
  label,
  children,
}: {
  open: boolean;
  onClose: (reason: "outside" | "escape" | "blur") => void;
  anchor: Anchor;
  placement?: Placement;
  /** At least as wide as the anchor (for dropdown lists). */
  matchWidth?: boolean;
  /** Move focus into the panel when it opens, and back when it closes. */
  focus?: boolean;
  className?: string;
  role?: string;
  /**
   * A click outside only closes it, without acting on what's under it, as with a menu (the
   * default for one). For panels whose page clicks would undo their context, such as the
   * register's selection.
   */
  modal?: boolean;
  label?: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  // A calendar moves focus in its own passive effect, before the parent's effect runs.
  // Remember the opener earlier and only when opening, not on panel rerenders.
  useLayoutEffect(() => {
    if (open) previousFocus.current = document.activeElement as HTMLElement | null;
  }, [open]);
  const [position, setPosition] = useState<{ left: number; top: number; minWidth?: number } | null>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }
    const place = () => {
      const el = ref.current;
      const rect = isRef(anchor)
        ? anchor.current?.getBoundingClientRect()
        : new DOMRect(anchor.x, anchor.y, 0, 0);
      if (!el || !rect) return;
      const gap = 4,
        margin = 8,
        w = el.offsetWidth,
        h = el.offsetHeight;
      // Start and end are the anchor's left and right edges, the other way round right to left;
      // "right-start" opens beside the anchor, on its left right to left.
      const rtl = getDirection() === "rtl";
      let left =
        placement === "bottom-end"
          ? rtl ? rect.left : rect.right - w
          : placement === "right-start"
            ? rtl ? rect.left - gap - w : rect.right + gap
            : rtl ? rect.right - w : rect.left;
      let top =
        placement === "top-start"
          ? rect.top - h - gap
          : placement === "right-start"
            ? rect.top
            : rect.bottom + gap;
      if (top + h > innerHeight - margin) top = Math.max(margin, rect.top - h - gap);
      if (top < margin) top = margin;
      left = Math.min(Math.max(margin, left), innerWidth - w - margin);
      setPosition({ left, top, minWidth: matchWidth ? rect.width : undefined });
    };
    place();
    // It follows its own size, and its anchor's (a search box that widens as it opens).
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    if (ref.current) observer?.observe(ref.current);
    if (isRef(anchor) && anchor.current) observer?.observe(anchor.current);
    addEventListener("resize", place);
    addEventListener("scroll", place, true);
    return () => {
      observer?.disconnect();
      removeEventListener("resize", place);
      removeEventListener("scroll", place, true);
    };
  }, [open, anchor, placement, matchWidth]);

  useEffect(() => {
    if (!open) return;
    const previous = previousFocus.current;
    const panel = ref.current;
    const frame = focus
      ? requestAnimationFrame(() => {
        const panel = ref.current;
        (
          panel?.querySelector<HTMLElement>("[data-autofocus]") ??
          panel?.querySelector<HTMLElement>(
            "[role=menuitem]:not([aria-disabled=true]),input,select,textarea,button:not(:disabled)",
          )
        )?.focus();
      })
      : undefined;
    // Popovers opened from inside this one (a category list in a filter panel) are portals
    // added after it: clicks in them are inside, and Escape closes only the topmost.
    const nested = (el: Element | null) =>
      !!el && !!ref.current && el !== ref.current && !!(ref.current.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING);
    // The press that closes a menu (or a modal panel) does only that, as a native menu's does: its
    // click doesn't go on to act on what's under it (reopen the menu from its button, pick a row,
    // tick a box). It's let go once the press ends, or after a second if it ends elsewhere.
    const takePress = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const swallow = (c: MouseEvent) => {
        c.stopPropagation();
        c.preventDefault();
      };
      const release = () => document.removeEventListener("click", swallow, true);
      document.addEventListener("click", swallow, true);
      document.addEventListener("mouseup", () => setTimeout(release), { capture: true, once: true });
      setTimeout(release, 1000);
    };
    const outside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (ref.current?.contains(target)) return;
      const onAnchor = isRef(anchor) && !!anchor.current?.contains(target);
      // A panel other than a menu leaves a press on its anchor to the anchor (a combobox's field
      // stays open as it's clicked in; a button may toggle it).
      if (onAnchor && role !== "menu") return;
      if (!onAnchor && nested((target as Element).closest?.(".popover") ?? null)) return;
      if (onAnchor || modal) takePress(e);
      close.current("outside");
    };
    // A menu also closes when the window loses focus (switching to another app), as native menus do.
    const blur = () => {
      if (role === "menu") close.current("blur");
    };
    // Capture phase, so an Escape that closes the popover does not also close a dialog under it.
    const key = (e: KeyboardEvent) => {
      const all = document.querySelectorAll(".popover");
      if (e.key === "Escape" && all[all.length - 1] === ref.current) {
        e.stopPropagation();
        e.preventDefault();
        close.current("escape");
      }
    };
    document.addEventListener("mousedown", outside, true);
    document.addEventListener("keydown", key, true);
    addEventListener("blur", blur);
    return () => {
      if (frame !== undefined) cancelAnimationFrame(frame);
      document.removeEventListener("mousedown", outside, true);
      document.removeEventListener("keydown", key, true);
      removeEventListener("blur", blur);
      // React can remove the portal and clear its ref before this cleanup.
      // A removed focused control leaves focus on body; retain outside focus.
      if (focus && previous?.isConnected &&
          (panel?.contains(document.activeElement) || document.activeElement === document.body))
        previous.focus();
    };
  }, [open, anchor, focus, role, modal]);

  if (!open) return null;
  return createPortal(
    <div
      ref={ref}
      className={`popover ${className}`}
      role={role}
      aria-label={label}
      style={{
        left: position?.left ?? 0,
        top: position?.top ?? 0,
        minWidth: position?.minWidth,
        visibility: position ? "visible" : "hidden",
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
