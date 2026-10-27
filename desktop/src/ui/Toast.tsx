import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { t } from "../i18n";

export type ToastInput = {
  message: string;
  tone?: "info" | "success" | "error";
  /** E.g. Undo, or Show in folder. */
  action?: { label: string; onClick: () => void };
  /** A next step offered before the action, such as asking the assistant. The toast stays longer. */
  offer?: { label: string; onClick: () => void };
  /** Milliseconds; errors and offers stay longer by default. */
  duration?: number;
};
type Item = ToastInput & { id: number };
const Context = createContext<(toast: ToastInput) => void>(() => {});
/** Shows a short message at the bottom of the window that goes away on its own. */
export const useToast = () => useContext(Context);

function ToastItem({ item, dismiss }: { item: Item; dismiss: (id: number) => void }) {
  const [paused, setPaused] = useState(false);
  // The pointer over a toast pauses it.
  const remaining = useRef(item.duration ?? (item.tone === "error" || item.offer ? 9000 : 5000));
  useEffect(() => {
    if (paused) return;
    const started = Date.now();
    const timer = setTimeout(() => dismiss(item.id), remaining.current);
    return () => {
      clearTimeout(timer);
      remaining.current -= Date.now() - started;
    };
  }, [paused, item.id, dismiss]);
  return (
    <div
      className={`toast ${item.tone ?? "info"}`}
      role={item.tone === "error" ? "alert" : "status"}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      <span>{item.message}</span>
      {item.offer && (
        <button
          className="toast-action"
          onClick={() => {
            item.offer!.onClick();
            dismiss(item.id);
          }}
        >
          {item.offer.label}
        </button>
      )}
      {item.action && (
        <button
          className="toast-action"
          onClick={() => {
            item.action!.onClick();
            dismiss(item.id);
          }}
        >
          {item.action.label}
        </button>
      )}
      <button className="icon-button" aria-label={t("Dismiss")} onClick={() => dismiss(item.id)}>
        <X size={14} />
      </button>
    </div>
  );
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const next = useRef(0);
  const dismiss = useCallback((id: number) => setItems((list) => list.filter((t) => t.id !== id)), []);
  const push = useCallback((toast: ToastInput) => {
    const id = ++next.current;
    // Newest last; keep at most three on screen.
    setItems((list) => [...list.slice(-2), { ...toast, id }]);
  }, []);
  return (
    <Context.Provider value={push}>
      {children}
      <div className="toasts">
        {items.map((item) => (
          <ToastItem key={item.id} item={item} dismiss={dismiss} />
        ))}
      </div>
    </Context.Provider>
  );
}
