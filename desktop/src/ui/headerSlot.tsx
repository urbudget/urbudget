import { createContext, useContext, type ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * The page header has two slots for a page's own controls: one just after its title (the report
 * groups), and one beside undo and redo (the register's import and export). App provides the
 * slots' elements, and a page portals into them. Undefined when there is no header around the
 * page at all, as when a test renders a page on its own.
 */
export type HeaderSlots = { title: HTMLElement | null; actions: HTMLElement | null };
export const HeaderSlotElements = createContext<HeaderSlots | undefined>(undefined);

/** Puts its children in the page header after the title, or with `actions` beside undo and redo;
 * with no header around it, in place. */
export function HeaderSlot({ children, actions = false }: { children: ReactNode; actions?: boolean }) {
  const slots = useContext(HeaderSlotElements);
  if (slots === undefined) return <>{children}</>;
  const slot = actions ? slots.actions : slots.title;
  return slot ? createPortal(children, slot) : null;
}
