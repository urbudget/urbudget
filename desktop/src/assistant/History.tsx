import { useEffect, useState, type KeyboardEvent } from "react";
import { Check, LoaderCircle, MessageSquareText, Trash2 } from "lucide-react";
import { formatDate } from "../format";
import { formatCount, t } from "../i18n";
import { Popover, type Anchor } from "../ui";
import { assistant, type Listing, type Usage } from "./api";

/** A moment ("2026-10-16T10:00:00Z") as the day it was here, in the budget's date format. */
const day = (stamp: string) => {
  const date = new Date(stamp);
  return formatDate(Number.isNaN(date.getTime()) ? stamp.slice(0, 10) : date.toLocaleDateString("en-CA"));
};

/** What a conversation has used, in one line; nothing when it has used nothing. No prices. */
export function usageLine(usage?: Usage) {
  if (!usage || !(usage.input || usage.output || usage.cached)) return "";
  return t("Tokens: {sent} sent ({cached} from cache), {received} received", {
    sent: formatCount(usage.input),
    cached: formatCount(usage.cached),
    received: formatCount(usage.output),
  });
}

/**
 * Every conversation of the budget, newest first, with a field to find one by its title. The
 * open one is marked and says what it has used, and can be deleted from here.
 */
export function History({
  open,
  onClose,
  anchor,
  current,
  usage,
  running,
  onOpen,
  onDelete,
}: {
  open: boolean;
  onClose: () => void;
  anchor: Anchor;
  /** The open conversation. */
  current: string | null;
  usage?: Usage;
  /** Its reply is still arriving: it can't be deleted yet. */
  running: boolean;
  onOpen: (id: string) => void;
  /** Asked for only: whoever hosts this asks the user first. */
  onDelete: () => void;
}) {
  /** Null until they have been read, and while they couldn't be. */
  const [threads, setThreads] = useState<Listing[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [query, setQuery] = useState("");
  const read = () => {
    setFailed(false);
    return assistant.threads().then(setThreads, () => setFailed(true));
  };
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setThreads(null);
    void read();
  }, [open]);

  const wanted = query.trim().toLocaleLowerCase();
  const found = (threads ?? []).filter((thread) => thread.title.toLocaleLowerCase().includes(wanted));
  const tokens = usageLine(usage);
  // The arrow keys go down the list from the search field, and back up to it.
  const move = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const stops = [...e.currentTarget.querySelectorAll<HTMLElement>("input, .assistant-history-item")];
    const next = stops[stops.indexOf(document.activeElement as HTMLElement) + (e.key === "ArrowDown" ? 1 : -1)];
    if (!next) return;
    e.preventDefault();
    next.focus();
  };
  return (
    <Popover open={open} onClose={() => onClose()} anchor={anchor} placement="bottom-end" modal label={t("Conversations")} className="assistant-history">
      <div className="assistant-history-box" onKeyDown={move}>
        <input
          type="search"
          data-autofocus
          aria-label={t("Search conversations")}
          placeholder={t("Search conversations")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="assistant-history-list">
          {threads === null ? (
            failed ? (
              <div className="assistant-history-note" role="alert">
                {t("The conversations couldn't be loaded.")}
                <button className="link-button" onClick={() => void read()}>
                  {t("Try again")}
                </button>
              </div>
            ) : (
              <div className="assistant-history-note" role="status">
                <LoaderCircle size={13} className="spin" aria-hidden /> {t("Loading…")}
              </div>
            )
          ) : threads.length === 0 ? (
            <div className="assistant-history-note">{t("No earlier conversations")}</div>
          ) : found.length === 0 ? (
            <div className="assistant-history-note">{t("No conversation matches.")}</div>
          ) : (
            found.map((thread) => {
              const here = thread.id === current;
              return (
                <button
                  key={thread.id}
                  className="assistant-history-item"
                  aria-current={here ? "true" : undefined}
                  onClick={() => {
                    onClose();
                    if (!here) onOpen(thread.id);
                  }}
                >
                  <span className="menu-icon">{here ? <Check size={14} aria-hidden /> : <MessageSquareText size={14} aria-hidden />}</span>
                  <span className="assistant-history-text">
                    <span dir="auto" title={thread.title}>
                      {thread.title}
                    </span>
                    <small>{day(thread.updated)}</small>
                    {here && tokens && <small>{tokens}</small>}
                  </span>
                </button>
              );
            })
          )}
        </div>
        {current && (
          <>
            <div className="menu-separator" role="separator" />
            <button
              className="menu-item danger"
              disabled={running}
              onClick={() => {
                onClose();
                onDelete();
              }}
            >
              <span className="menu-icon">
                <Trash2 size={14} aria-hidden />
              </span>
              <span>{t("Delete this conversation")}</span>
            </button>
          </>
        )}
      </div>
    </Popover>
  );
}
