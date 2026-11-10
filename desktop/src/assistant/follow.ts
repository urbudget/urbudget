import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

export const reducedMotion = () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Where a scrolling box stands: how far down it is, how tall what's in it is, and how much of that shows. */
export type Scroll = { top: number; height: number; view: number };

/** At the end, give or take a pixel of rounding. */
export const atEnd = (s: Scroll) => s.height - s.top - s.view < 2;

/**
 * Whether a conversation still follows its end once it has scrolled: reaching the end follows
 * again, any move up stops following, and anything else (a move down that stops short of the
 * end) leaves it as it was.
 */
export function following(was: { following: boolean; top: number }, now: Scroll): boolean {
  if (atEnd(now)) return true;
  if (now.top < was.top) return false;
  return was.following;
}

/**
 * Keeps the conversation in `box` at its end while new text arrives, for as long as the user is
 * reading the end. Scrolling up by any means (the wheel, a key, the scrollbar) stops that, and
 * `away` then says there is more below to jump to. `content` is what grows inside the box, there
 * while `shown`.
 */
export function useFollow(box: RefObject<HTMLElement | null>, content: RefObject<HTMLElement | null>, shown: boolean) {
  const pinned = useRef(true);
  /** Where the box was last seen or put, to tell a move up from a move down. */
  const top = useRef(0);
  /** The next change of height is the user's own doing, not new text. */
  const owned = useRef(false);
  const [away, setAway] = useState(false);

  const measure = (): Scroll | null => {
    const el = box.current;
    return el ? { top: el.scrollTop, height: el.scrollHeight, view: el.clientHeight } : null;
  };
  const settle = (follows: boolean, now: Scroll | null = measure()) => {
    pinned.current = follows;
    setAway(!follows && !!now && !atEnd(now));
  };
  const toEnd = () => {
    const el = box.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    top.current = el.scrollTop;
  };
  /** Something new arrived: stay at the end, if that's where the user is. */
  const follow = useCallback(() => {
    const now = measure();
    if (!now) return;
    // A move up whose scroll event hasn't been heard yet counts already: the box is higher than
    // it was left, and not because what's in it got shorter.
    if (pinned.current && now.top < top.current - 1 && !atEnd(now)) pinned.current = false;
    if (pinned.current) toEnd();
    settle(pinned.current);
  }, []);

  useEffect(() => {
    if (!shown || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (!owned.current) return follow();
      // Taller by the user's own click: what they clicked stays where it is, and following goes
      // on only if the end is still in view.
      owned.current = false;
      const now = measure();
      settle(pinned.current && !!now && atEnd(now), now);
    });
    // Text revealed inside a reply makes the content taller; a taller message field or a shorter
    // window makes the box shorter.
    if (content.current) observer.observe(content.current);
    if (box.current) observer.observe(box.current);
    return () => observer.disconnect();
  }, [shown, follow]);

  return {
    /** The user has left the end and there is more below it. */
    away,
    follow,
    onScroll: () => {
      const now = measure();
      if (!now) return;
      settle(following({ following: pinned.current, top: top.current }, now), now);
      top.current = now.top;
    },
    /** The wheel says so before the box has moved, so text arriving meanwhile doesn't pull it back. */
    onWheel: (e: { deltaY: number }) => {
      if (e.deltaY < 0 && (box.current?.scrollTop ?? 0) > 0) settle(false);
    },
    /** Goes to the end and follows again, at once: a conversation just opened, a message just sent. */
    land: () => {
      pinned.current = true;
      setAway(false);
      toEnd();
    },
    /** "Jump to latest": the same, moving there smoothly. */
    jump: () => {
      pinned.current = true;
      setAway(false);
      const el = box.current;
      el?.scrollTo?.({ top: el.scrollHeight, behavior: reducedMotion() ? "auto" : "smooth" });
    },
    /**
     * The click being handled changes the conversation's height by itself (a card's rows, its
     * editor, the lookups behind a reply): that is the user's doing, not new text to follow.
     */
    own: () => {
      if (typeof ResizeObserver === "undefined") return;
      owned.current = true;
      // Nothing changed size after all: whatever grows next isn't theirs.
      requestAnimationFrame(() => requestAnimationFrame(() => (owned.current = false)));
    },
  };
}
