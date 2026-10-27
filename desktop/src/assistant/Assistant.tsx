import { Component, Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ErrorInfo, type ReactNode } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  FileSpreadsheet,
  FileText,
  Flag,
  History,
  Image,
  ListChecks,
  LoaderCircle,
  MessageSquareText,
  Paperclip,
  Square,
  SquarePen,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react";
import { api } from "../bridge";
import { errorText, monthName } from "../format";
import { keyLabel } from "../platform";
import { today, type Budget, type Summary } from "../types";
import { Menu, useToast } from "../ui";
import { ACCEPT, assistant, fileData, type Attachment, type Card, type Entry, type Listing, type Proposal, type Revision, type Screen, type Status } from "./api";
import { Markdown } from "./Markdown";
import { describe, logError } from "./errors";
import { ProposalCard } from "./ProposalCard";
import { printReport, ReportPreview } from "./Report";
import { ReplyReport } from "./ReplyReport";
import { getDirection, msg, msgc, t, tn, tnc, useLanguage } from "../i18n";
import "./assistant.css";

export { Markdown } from "./Markdown";

const store = {
  get: (key: string) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set: (key: string, value: string | null) => {
    try {
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    } catch {
      /* not remembered */
    }
  },
};
export const WIDTH = { min: 320, max: 640, default: 380 };
/** The panel's width, remembered between sessions. */
export function useAssistantWidth() {
  const [width, setWidthState] = useState(() => {
    const saved = Number(store.get("lb:assistant:width"));
    return saved >= WIDTH.min && saved <= WIDTH.max ? saved : WIDTH.default;
  });
  const setWidth = (value: number) => {
    const next = Math.round(Math.min(WIDTH.max, Math.max(WIDTH.min, value)));
    setWidthState(next);
    store.set("lb:assistant:width", String(next));
  };
  return { width, setWidth };
}

/** Something an entry point asks the panel to do when it opens. */
export type Request = {
  /** Changes each time, so the same request can be made twice. */
  key: number;
  text?: string;
  selected?: string[];
  files?: File[];
  /** Send `text` right away. */
  send?: boolean;
};

const PAGE_NAMES: Record<string, string> = {
  budget: msg("Budget"),
  transactions: msg("Transactions"),
  scheduled: msgc("page", "Scheduled"),
  loans: msg("Loan details"),
  reports: msg("Reports"),
  payees: msg("Payees & rules"),
  settings: msg("Settings"),
};

/** Keeps a problem in the assistant from taking down the rest of the app. */
export class AssistantBoundary extends Component<
  { children: ReactNode; budgetId: string; onClose: () => void },
  { detail: string | null; attempt: number }
> {
  state = { detail: null as string | null, attempt: 0 };
  static getDerivedStateFromError(error: unknown) {
    return { detail: describe(error) };
  }
  componentDidCatch(error: unknown, info: ErrorInfo) {
    logError(error, `in the assistant${info.componentStack ?? ""}`);
  }
  render() {
    const retry = (fresh: boolean) => () => {
      if (fresh) store.set(`lb:assistant:thread:${this.props.budgetId}`, null);
      this.setState((s) => ({ detail: null, attempt: s.attempt + 1 }));
    };
    if (this.state.detail === null) return <Fragment key={this.state.attempt}>{this.props.children}</Fragment>;
    return (
      <aside className="assistant" aria-label={t("Assistant")}>
        <header className="assistant-head">
          <strong>{t("Assistant")}</strong>
          <div className="assistant-tools">
            <button className="icon-button" aria-label={t("Close the assistant")} title={keyLabel(t("Close (Ctrl+J)"))} onClick={this.props.onClose}>
              <X size={16} />
            </button>
          </div>
        </header>
        <div className="assistant-signin" role="alert">
          <h3>{t("The assistant ran into a problem")}</h3>
          <p>{t("The rest of the app is fine, and nothing in your budget changed. The details are saved in errors.log in the app's data folder.")}</p>
          <div className="assistant-signin-links">
            <button className="primary" onClick={retry(false)}>
              {t("Reload the assistant")}
            </button>
            <button className="secondary" onClick={retry(true)}>
              {t("New conversation")}
            </button>
          </div>
          <details className="crash-details">
            <summary>{t("Details")}</summary>
            <pre>{this.state.detail}</pre>
          </details>
        </div>
      </aside>
    );
  }
}

/** Starting points that fit the budget as it is right now. */
export function suggestions(budget: Budget, summary: Summary, screen: Omit<Screen, "selected" | "today">): string[] {
  const out: string[] = [];
  const overspent = summary.categories.filter((c) => c.available < 0).length;
  if (overspent) out.push(tn(overspent, "Cover the overspent category", "Cover the {count} overspent categories"));
  const waiting = budget.transactions.filter((tx) => tx.approved === false).length;
  if (waiting) out.push(tn(waiting, "Categorize the new transaction", "Categorize the {count} new transactions"));
  const underfunded = summary.categories.filter((c) => (c.needed ?? 0) > 0).length;
  if (underfunded) out.push(t("Fund what my targets need this month"));
  const account = budget.accounts.find((a) => a.id === screen.accountId);
  if (screen.page === "transactions" && account) out.push(t("Anything unusual in {account} lately?", { account: account.name }));
  out.push(t("How is {month} going?", { month: monthName(screen.month || today().slice(0, 7)) }));
  return out.slice(0, 4);
}

/** What the model is working on, from its reasoning summary: the latest heading or finished sentence. */
export function thinkingTitle(text: string): string {
  const heads = [...text.matchAll(/\*\*([^*\n]{2,80}?)\*\*/g)].map((m) => m[1].trim());
  if (heads.length) return heads[heads.length - 1];
  const sentences = text.trim().split(/(?<=[.!?])\s+/).filter((s) => /[.!?]$/.test(s));
  const last = sentences[sentences.length - 1] ?? "";
  return last.length > 80 ? `${last.slice(0, 79).trimEnd()}…` : last;
}

const reducedMotion = () => typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Reveals streamed text at an even pace instead of in network-sized bursts: each frame shows a
 * share of what is waiting, so it keeps up with a fast stream and eases out at the end.
 */
function useReveal(text: string, animate: boolean) {
  const [count, setCount] = useState(() => (animate && !reducedMotion() ? 0 : text.length));
  const shown = useRef(count);
  const target = useRef(text.length);
  target.current = text.length;
  useEffect(() => {
    if (shown.current >= text.length) {
      if (shown.current > text.length) setCount((shown.current = text.length));
      return;
    }
    let frame = 0;
    const tick = () => {
      const waiting = target.current - shown.current;
      if (waiting <= 0) return;
      shown.current += Math.max(1, Math.ceil(waiting / 12));
      setCount(shown.current);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [text]);
  return text.slice(0, count);
}

/**
 * One reply. A live reply keeps the same element when it finishes, so nothing jumps; its Report
 * reporting button holds its place meanwhile. Submitting a report stays inside the app.
 */
function Reply({ text, live }: { text: string; live: boolean }) {
  const [reporting, setReporting] = useState(false);
  const [animate] = useState(live);
  const shown = useReveal(text, animate);
  const typing = live || shown.length < text.length;
  return (
    <>
      <div className={`assistant-reply${typing ? " typing" : ""}`}>
        <Markdown text={shown} streaming={typing} />
      </div>
      <button
        className={`assistant-flag${typing ? " waiting" : ""}`}
        title={t("Report this reply to UrBudget's developer")}
        tabIndex={typing ? -1 : 0}
        disabled={typing}
        onClick={() => setReporting(true)}
      >
        <Flag size={11} aria-hidden />
        {t("Report")}
      </button>
      {reporting && <ReplyReport text={text} close={() => setReporting(false)} />}
    </>
  );
}

type Step = { text: string; failed: boolean };
function StepLine({ step }: { step: Step }) {
  return (
    <div className={`assistant-step${step.failed ? " failed" : ""}`}>
      {step.failed ? <TriangleAlert size={12} aria-hidden /> : <Check size={12} aria-hidden />}
      <span>{step.text}</span>
    </div>
  );
}
/** The lookups behind a reply: the latest one, with the rest a click away. */
function Steps({ steps }: { steps: Step[] }) {
  const [open, setOpen] = useState(false);
  if (steps.length === 1) return <StepLine step={steps[0]} />;
  return (
    <div className="assistant-steps">
      {(open ? steps : steps.slice(-1)).map((s, i) => (
        <StepLine key={i} step={s} />
      ))}
      <button className="assistant-steps-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? t("Fewer") : tn(steps.length - 1, "{count} more", "{count} more")}
        <ChevronDown size={12} aria-hidden />
      </button>
    </div>
  );
}

const fileIcon = (a: Attachment) =>
  a.kind === "image" ? <Image size={13} aria-hidden /> : a.kind === "sheet" || a.kind === "csv" ? <FileSpreadsheet size={13} aria-hidden /> : <FileText size={13} aria-hidden />;
const planLabel = (s: Status) =>
  s.provider === "chatgpt" ? t("Using ChatGPT plan") : s.provider === "openai" ? t("Using your OpenAI key") : t("Using your Anthropic key");

/** The latest proposal still waiting for the user, whose rows the page highlights. */
const pending = (entries: Entry[]) =>
  [...entries].reverse().find((e): e is Extract<Entry, { kind: "proposal" }> => e.kind === "proposal" && e.proposal.status === "proposed")
    ?.proposal;
const same = (a: Entry[], b: Entry[]) => a.length === b.length && JSON.stringify(a) === JSON.stringify(b);

function SignIn({ status, onStatus, onOpenSettings }: { status: Status; onStatus: (s: Status) => void; onOpenSettings: () => void }) {
  const [busy, setBusy] = useState(status.signingIn);
  const [error, setError] = useState("");
  const [why, setWhy] = useState(false);
  if (status.provider !== "chatgpt")
    return (
      <div className="assistant-signin">
        <span className="assistant-badge">
          <MessageSquareText size={20} aria-hidden />
        </span>
        <h3>{t("Add your API key")}</h3>
        <p>{t("The assistant is set to use your own {provider} API key. Add it in Settings.", { provider: status.provider === "openai" ? "OpenAI" : "Anthropic" })}</p>
        <button className="primary" onClick={onOpenSettings}>
          {t("Open settings")}
        </button>
      </div>
    );
  const signIn = async () => {
    setBusy(true);
    setError("");
    try {
      onStatus(await assistant.signIn());
    } catch (e) {
      setError(errorText(e));
      onStatus(await assistant.status());
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="assistant-signin">
      <span className="assistant-badge">
        <MessageSquareText size={20} aria-hidden />
      </span>
      <h3>{t("Sign in to use the assistant")}</h3>
      <p>{t("It runs on your ChatGPT Plus or Pro plan. Your budget stays on this computer; only what you ask about is sent.")}</p>
      {busy ? (
        <div className="assistant-waiting" role="status">
          <LoaderCircle size={15} className="spin" aria-hidden />{" "}
          {t("Finish signing in in your browser…")}
          <button className="link-button" onClick={() => void assistant.cancelSignIn()}>
            {t("Cancel")}
          </button>
        </div>
      ) : (
        <button className="primary" onClick={() => void signIn()}>
          {t("Continue with ChatGPT")}
        </button>
      )}
      {error && (
        <p className="assistant-error" role="alert">
          {error}
        </p>
      )}
      <div className="assistant-signin-links">
        <button className="link-button" onClick={() => setWhy(!why)} aria-expanded={why}>
          {t("What gets sent?")}
        </button>
        <button className="link-button" onClick={onOpenSettings}>
          {t("Use an API key instead")}
        </button>
      </div>
      {why && (
        <p className="assistant-why">
          {t("Your messages, what's on screen, the files you attach, and the budget details the assistant looks up to answer you. Your budget file, conversation history and sign-in are kept on this computer. OpenAI's terms and privacy policy govern how it processes and retains what you send. Nothing in the budget changes until you apply a change yourself.")}
        </p>
      )}
    </div>
  );
}

export function AssistantPanel({
  budget,
  budgetId,
  summary,
  screen,
  request,
  width,
  onResize,
  onClose,
  onOpenSettings,
  onApply,
  onUndo,
  canUndo,
  onReviewImport,
  onHighlight,
  onWorking,
}: {
  budget: Budget;
  budgetId: string;
  summary: Summary;
  screen: Omit<Screen, "selected" | "today">;
  request: Request | null;
  width: number;
  onResize: (width: number) => void;
  onClose: () => void;
  onOpenSettings: () => void;
  /** Applies a proposal to the budget; resolves to the card as it is afterwards. */
  onApply: (threadId: string, p: Proposal) => Promise<Proposal>;
  onUndo: (threadId: string, p: Proposal) => Promise<Proposal>;
  canUndo: (p: Proposal) => boolean;
  onReviewImport: (threadId: string, p: Proposal) => void;
  onHighlight: (highlight: Card["highlight"] | null) => void;
  /** Whether there's a message being written, attached to or sent, which closing would lose. */
  onWorking?: (working: boolean) => void;
}) {
  const toast = useToast();
  const threadKey = `lb:assistant:thread:${budgetId}`;
  const [status, setStatus] = useState<Status | null>(null);
  const [threadId, setThreadId] = useState<string | null>(() => store.get(threadKey));
  const [entries, setEntries] = useState<Entry[]>([]);
  /** Entries from this index on arrived while the panel was open, and fade in. */
  const [freshFrom, setFreshFrom] = useState(0);
  const [running, setRunning] = useState(false);
  /** The reply streaming in right now, before it is complete. */
  const [live, setLiveState] = useState<string | null>(null);
  const liveText = useRef<string | null>(null);
  const setLive = (text: string | null) => {
    liveText.current = text;
    setLiveState(text);
  };
  const [thinking, setThinking] = useState("");
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [uploading, setUploading] = useState(0);
  const [selected, setSelected] = useState<string[]>([]);
  const [problem, setProblem] = useState("");
  const [threads, setThreads] = useState<Listing[]>([]);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [preview, setPreview] = useState<{ threadId: string; id: string } | null>(null);
  const [away, setAway] = useState(false);
  const current = useRef(threadId);
  current.current = threadId;
  const historyButton = useRef<HTMLButtonElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const feed = useRef<HTMLDivElement>(null);
  const handled = useRef<number | null>(null);
  /** Follow new content while the user is reading the end of the conversation. */
  const stick = useRef(true);

  const remember = (id: string | null) => {
    setThreadId(id);
    current.current = id;
    store.set(threadKey, id);
  };
  /** Loads a conversation; `opening` means none of it is new, so nothing fades in. */
  const load = useCallback(async (id: string | null, opening = false) => {
    if (!id) {
      setEntries([]);
      setFreshFrom(0);
      setRunning(false);
      setLive(null);
      return;
    }
    try {
      const view = await assistant.thread(id);
      if (current.current !== id) return;
      setEntries((prev) => (same(prev, view.entries) ? prev : view.entries));
      if (opening) {
        setFreshFrom(view.entries.length);
        stick.current = true;
      }
      setRunning(view.running);
      if (!view.running) {
        setLive(null);
        setThinking("");
      }
    } catch {
      remember(null);
      setEntries([]);
    }
  }, []);

  useEffect(() => {
    void assistant.status().then(setStatus).catch((e) => setProblem(errorText(e)));
    const refresh = () => void assistant.status().then(setStatus).catch(() => {});
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
  // Another budget has its own conversations.
  useEffect(() => {
    const id = store.get(threadKey);
    remember(id);
    void load(id, true);
  }, [budgetId, load]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void assistant
      .onEvent((e) => {
        if (e.threadId !== current.current) return;
        switch (e.type) {
          case "started":
            setRunning(true);
            setLive(null);
            setThinking("");
            break;
          case "thinking":
            setThinking((t) => t + e.text);
            break;
          case "delta":
            setLive((liveText.current ?? "") + e.text);
            break;
          case "message":
            setLive(null);
            setThinking("");
            setEntries((list) => [...list, { kind: "assistant", text: e.text }]);
            break;
          case "step": {
            // A reply cut short by a lookup ends here, keeping its place and element.
            const text = liveText.current;
            setLive(null);
            setThinking("");
            setEntries((list) => [...list, ...(text ? [{ kind: "assistant" as const, text }] : []), { kind: "step", text: e.text, failed: e.failed }]);
            break;
          }
          case "proposal":
            setEntries((list) => [...list, { kind: "proposal", proposal: e.proposal }]);
            break;
          case "error":
            setLive(null);
            setThinking("");
            setEntries((list) => [...list, { kind: "error", text: e.text, code: e.code }]);
            break;
          case "done":
            setRunning(false);
            setThinking("");
            void load(e.threadId).then(() => setLive(null));
            void assistant.status().then(setStatus).catch(() => {});
            break;
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load]);

  // Stay at the end while new content arrives, unless the user scrolled up to read.
  const follow = () => {
    if (stick.current && body.current) body.current.scrollTop = body.current.scrollHeight;
  };
  useLayoutEffect(follow);
  useEffect(() => {
    if (!feed.current || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(follow);
    observer.observe(feed.current);
    return () => observer.disconnect();
  }, [status?.ready]);
  const onScroll = () => {
    const el = body.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    setAway(!stick.current);
  };
  const jump = () => {
    stick.current = true;
    setAway(false);
    body.current?.scrollTo?.({ top: body.current.scrollHeight, behavior: reducedMotion() ? "auto" : "smooth" });
  };

  const waiting = pending(entries);
  const working = running || uploading > 0 || !!draft.trim() || attachments.length > 0;
  useEffect(() => {
    onWorking?.(working);
  }, [working]);
  useEffect(() => () => onWorking?.(false), []);
  useEffect(() => {
    onHighlight(waiting ? waiting.card.highlight : null);
  }, [waiting]);
  useEffect(() => () => onHighlight(null), []);

  const upload = async (files: File[]) => {
    const room = 5 - attachments.length;
    if (files.length > room) toast({ message: t("Attach up to 5 files at a time."), tone: "error" });
    for (const file of files.slice(0, Math.max(0, room))) {
      setUploading((n) => n + 1);
      try {
        const ready = await fileData(file);
        const saved = await assistant.attach(ready.name, ready.data);
        setAttachments((list) => [...list, saved]);
      } catch (e) {
        toast({ message: errorText(e), tone: "error" });
      } finally {
        setUploading((n) => n - 1);
      }
    }
    input.current?.focus();
  };

  const send = async (text: string) => {
    if (!status?.ready || running || uploading) return;
    const message = text.trim();
    if (!message && !attachments.length) return;
    setProblem("");
    const sent = { text: message, attachments, selected };
    stick.current = true;
    setAway(false);
    setEntries((list) => [...list, { kind: "user", text: message, attachments }]);
    setDraft("");
    setAttachments([]);
    setSelected([]);
    setRunning(true);
    setLive(null);
    setThinking("");
    try {
      const id = await assistant.send(budget, current.current, message, sent.attachments.map((a) => a.id), {
        ...screen,
        selected: sent.selected,
        today: today(),
      });
      if (id !== current.current) remember(id);
      await load(id);
    } catch (e) {
      setRunning(false);
      setEntries((list) => list.slice(0, -1));
      setDraft(sent.text);
      setAttachments(sent.attachments);
      setSelected(sent.selected);
      setProblem(errorText(e));
    }
  };

  // Entry points: "Ask about these", a dropped file, a question from the inspector.
  useEffect(() => {
    if (!request || handled.current === request.key || !status) return;
    handled.current = request.key;
    if (request.selected?.length) setSelected(request.selected);
    if (request.files?.length) void upload(request.files);
    if (request.text && request.send && status.ready) void send(request.text);
    else if (request.text) setDraft(request.text);
    setTimeout(() => input.current?.focus());
  }, [request, status]);
  // The message field is as tall as its text (up to 160px) however the text got there: typed,
  // filled in by a request from elsewhere (the import's "Fix with the assistant"), or put back
  // after a send that failed. It appears once the assistant is ready, with any text already in it.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [draft, status?.ready]);

  const newConversation = () => {
    if (running) return;
    remember(null);
    setEntries([]);
    setFreshFrom(0);
    setLive(null);
    setThinking("");
    setProblem("");
    setTimeout(() => input.current?.focus());
  };
  const openHistory = async () => {
    setThreads(await assistant.threads().catch(() => []));
    setHistoryOpen(true);
  };
  const replace = (p: Proposal) =>
    setEntries((list) => list.map((e) => (e.kind === "proposal" && e.proposal.id === p.id ? { kind: "proposal", proposal: p } : e)));
  const act = <A extends unknown[]>(work: (...args: A) => Promise<Proposal>) => async (...args: A) => {
    try {
      replace(await work(...args));
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    }
  };
  const retry = async () => {
    if (!current.current) return;
    setProblem("");
    try {
      await assistant.retry(budget, current.current, { ...screen, selected: [], today: today() });
      setRunning(true);
    } catch (e) {
      setProblem(errorText(e));
    }
  };

  const contextLabel =
    selected.length > 0
      ? tnc("items", selected.length, "{count} selected", "{count} selected")
      : screen.page === "budget"
        ? t("{month} · Budget", { month: monthName(screen.month) })
        : screen.page === "transactions"
          ? (budget.accounts.find((a) => a.id === screen.accountId)?.name ?? t("All transactions"))
          : PAGE_NAMES[screen.page]
            ? t(PAGE_NAMES[screen.page])
            : "";

  const renderEntry = (e: Entry, last: boolean): ReactNode => {
    switch (e.kind) {
      case "user":
        return (
          <div className="assistant-user">
            {e.attachments.length > 0 && (
              <div className="assistant-files">
                {e.attachments.map((a) => (
                  <span key={a.id} className="assistant-file" title={a.name}>
                    {fileIcon(a)}
                    {a.name}
                  </span>
                ))}
              </div>
            )}
            {e.text && <p>{e.text}</p>}
          </div>
        );
      case "assistant":
        return <Reply text={e.text} live={false} />;
      case "step":
        return null;
      case "proposal": {
        const p = e.proposal;
        const id = current.current!;
        return (
          <ProposalCard
            proposal={p}
            budget={budget}
            canUndo={canUndo(p)}
            onApply={act(() => onApply(id, p))}
            onRevise={act((revision: Revision) => assistant.revise(budget, id, p.id, revision))}
            onDiscard={act(() => assistant.mark(id, p.id, "discarded"))}
            onUndo={act(() => onUndo(id, p))}
            onReview={() => onReviewImport(id, p)}
            onPreview={() => setPreview({ threadId: id, id: p.id })}
            onPrint={async () => {
              try {
                await printReport(await assistant.report(id, p.id));
              } catch (err) {
                toast({ message: errorText(err), tone: "error" });
              }
            }}
            onSheet={async (open) => {
              try {
                const path = await assistant.spreadsheet(budget, id, p.id, open);
                if (open || !path) return;
                const name = path.split(/[\\/]/).pop() ?? path;
                const report = p.kind === "report";
                toast({
                  message: t("Saved \u201c{name}\u201d.", { name }),
                  action: report
                    ? { label: t("Open"), onClick: () => void assistant.spreadsheet(budget, id, p.id, true).catch((e) => toast({ message: errorText(e), tone: "error" })) }
                    : { label: t("Show in folder"), onClick: () => void api.reveal(path).catch((e) => toast({ message: errorText(e), tone: "error" })) },
                });
              } catch (err) {
                toast({ message: errorText(err), tone: "error" });
              }
            }}
            onSave={async () => {
              try {
                const path = await assistant.saveImport(id, p.id);
                if (!path) return;
                toast({
                  message: t("Saved the converted file."),
                  action: { label: t("Show in folder"), onClick: () => void api.reveal(path).catch((e) => toast({ message: errorText(e), tone: "error" })) },
                });
              } catch (err) {
                toast({ message: errorText(err), tone: "error" });
              }
            }}
          />
        );
      }
      case "error": {
        const actionable = last && !running;
        const chatgpt = status?.provider === "chatgpt";
        return (
          <div className="assistant-problem" role="alert">
            <strong>{e.code === "limit" ? t("Usage limit reached") : e.code === "refused" ? t("Declined") : t("Something went wrong")}</strong>
            <p>{e.text}</p>
            {actionable && (
              <div className="assistant-problem-actions">
                {e.code === "limit" && chatgpt ? (
                  <button className="primary small" onClick={() => void assistant.open("usage")}>
                    {t("Manage usage")}
                  </button>
                ) : e.code === "signIn" ? (
                  chatgpt ? (
                    <button
                      className="primary small"
                      onClick={() =>
                        void assistant
                          .signIn()
                          .then(setStatus)
                          .catch((err) => toast({ message: errorText(err), tone: "error" }))
                      }
                    >
                      {t("Continue with ChatGPT")}
                    </button>
                  ) : (
                    <button className="primary small" onClick={onOpenSettings}>
                      {t("Open settings")}
                    </button>
                  )
                ) : e.code === "model" || e.code === "notEligible" ? (
                  <button className="secondary small" onClick={onOpenSettings}>
                    {t("Open settings")}
                  </button>
                ) : (
                  <button className="secondary small" onClick={() => void retry()}>
                    {t("Try again")}
                  </button>
                )}
              </div>
            )}
          </div>
        );
      }
      case "stopped":
        return (
          <div className="assistant-step">
            <Square size={10} aria-hidden /> <span>{t("Stopped")}</span>
          </div>
        );
    }
  };

  // Consecutive lookups share one line; everything keeps its index as its key, so a live reply
  // that finishes stays the same element.
  const feedItems: ReactNode[] = [];
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i];
    const fresh = i >= freshFrom ? " enter" : "";
    if (e.kind === "step") {
      const steps: Step[] = [];
      let j = i;
      while (j < entries.length && entries[j].kind === "step") {
        const s = entries[j] as Extract<Entry, { kind: "step" }>;
        steps.push({ text: s.text, failed: s.failed });
        j++;
      }
      feedItems.push(
        <div key={`s${i}`} className={`assistant-item${fresh}`}>
          <Steps steps={steps} />
        </div>,
      );
      i = j - 1;
      continue;
    }
    feedItems.push(
      <div key={i} className={`assistant-item${fresh}`}>
        {renderEntry(e, i === entries.length - 1)}
      </div>,
    );
  }
  if (live !== null)
    feedItems.push(
      <div key={entries.length} className="assistant-item enter">
        <Reply text={live} live />
      </div>,
    );
  else if (running) {
    const title = thinkingTitle(thinking);
    feedItems.push(
      <div key="activity" className="assistant-item assistant-activity enter" role="status">
        <span className="assistant-dots" aria-hidden>
          <i />
          <i />
          <i />
        </span>
        <span className="assistant-activity-text">{title || t("Thinking")}</span>
      </div>,
    );
  }

  const language = useLanguage();
  const ideas = useMemo(() => suggestions(budget, summary, screen), [budget, summary, screen.page, screen.month, screen.accountId, language]);
  const canSend = !!status?.ready && !running && !uploading && (!!draft.trim() || attachments.length > 0);

  return (
    <aside
      className={`assistant${dragging ? " dragging" : ""}`}
      aria-label={t("Assistant")}
      onDragOver={(e) => {
        if (!status?.ready || !e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDragging(false);
      }}
      onDrop={(e) => {
        if (!status?.ready) return;
        e.preventDefault();
        setDragging(false);
        void upload([...e.dataTransfer.files]);
      }}
    >
      <div
        className="assistant-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label={t("Resize assistant")}
        tabIndex={0}
        onPointerDown={(e) => {
          e.preventDefault();
          const handle = e.currentTarget;
          handle.setPointerCapture(e.pointerId);
          // The panel is on the right, or on the left right to left.
          const box = handle.parentElement!.getBoundingClientRect();
          const rtl = getDirection() === "rtl";
          const move = (m: PointerEvent) => onResize(rtl ? m.clientX - box.left : box.right - m.clientX);
          const up = () => {
            handle.removeEventListener("pointermove", move);
            handle.removeEventListener("pointerup", up);
          };
          handle.addEventListener("pointermove", move);
          handle.addEventListener("pointerup", up);
        }}
        onDoubleClick={() => onResize(WIDTH.default)}
        onKeyDown={(e) => {
          const wider = getDirection() === "rtl" ? "ArrowRight" : "ArrowLeft";
          if (e.key === "ArrowLeft" || e.key === "ArrowRight") onResize(width + (e.key === wider ? 16 : -16));
        }}
      />
      <header className="assistant-head">
        <strong>{t("Assistant")}</strong>
        {status?.ready && contextLabel && <span className="assistant-context">{contextLabel}</span>}
        <div className="assistant-tools">
          {status?.ready && (
            <>
              <button ref={historyButton} className="icon-button" aria-label={t("Conversations")} title={t("Conversations")} onClick={() => void openHistory()}>
                <History size={15} />
              </button>
              <button className="icon-button" aria-label={t("New conversation")} title={t("New conversation")} disabled={running} onClick={newConversation}>
                <SquarePen size={15} />
              </button>
            </>
          )}
          <button className="icon-button" aria-label={t("Close the assistant")} title={keyLabel(t("Close (Ctrl+J)"))} onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <Menu
          open={historyOpen}
          onClose={() => setHistoryOpen(false)}
          anchor={historyButton}
          placement="bottom-end"
          label={t("Conversations")}
          items={[
            ...(threads.length
              ? threads.slice(0, 15).map((thread) => ({
                  label: thread.title,
                  icon: thread.id === threadId ? <Check size={14} /> : <MessageSquareText size={14} />,
                  onSelect: () => {
                    remember(thread.id);
                    setLive(null);
                    setThinking("");
                    void load(thread.id, true);
                  },
                }))
              : [{ label: t("No earlier conversations"), disabled: true, onSelect: () => {} }]),
            ...(threadId
              ? [
                  "separator" as const,
                  {
                    label: t("Delete this conversation"),
                    icon: <Trash2 size={14} />,
                    danger: true,
                    disabled: running,
                    onSelect: () => {
                      const id = threadId;
                      newConversation();
                      void assistant.deleteThread(id).catch((e) => toast({ message: errorText(e), tone: "error" }));
                    },
                  },
                ]
              : []),
          ]}
        />
      </header>

      {!status ? (
        <div className="assistant-loading">{problem || <LoaderCircle size={18} className="spin" />}</div>
      ) : !status.ready ? (
        <SignIn status={status} onStatus={setStatus} onOpenSettings={onOpenSettings} />
      ) : (
        <>
          <div className="assistant-body" ref={body} onScroll={onScroll}>
            <div className="assistant-feed" ref={feed} aria-live="polite">
              {entries.length === 0 && !running ? (
                <div className="assistant-welcome">
                  <h3>{t("What would you like to do?")}</h3>
                  <div className="assistant-ideas">
                    {ideas.map((idea) => (
                      <button key={idea} className="assistant-idea" onClick={() => void send(idea)}>
                        {idea}
                      </button>
                    ))}
                  </div>
                  <div className="assistant-drop">
                    <Paperclip size={14} aria-hidden /> {t("Drop a statement, receipt or spreadsheet here")}
                  </div>
                </div>
              ) : (
                feedItems
              )}
            </div>
            {away && entries.length > 0 && (
            <button className="assistant-jump" aria-label={t("Jump to the latest")} title={t("Jump to the latest")} onClick={jump}>
              <ArrowDown size={14} />
            </button>
          )}
          </div>
          <div className="assistant-compose">
            {problem && (
              <p className="assistant-error" role="alert">
                {problem}
              </p>
            )}
            {(selected.length > 0 || attachments.length > 0 || uploading > 0) && (
              <div className="assistant-files">
                {selected.length > 0 && (
                  <span className="assistant-file selection">
                    <ListChecks size={13} aria-hidden />
                    {tn(selected.length, "{count} selected transaction", "{count} selected transactions")}
                    <button className="assistant-file-remove" aria-label={t("Don't attach the selection")} onClick={() => setSelected([])}>
                      <X size={12} />
                    </button>
                  </span>
                )}
                {attachments.map((a) => (
                  <span key={a.id} className="assistant-file" title={a.name}>
                    {fileIcon(a)}
                    {a.name}
                    <button
                      className="assistant-file-remove"
                      aria-label={t("Remove {name}", { name: a.name })}
                      onClick={() => setAttachments((list) => list.filter((x) => x.id !== a.id))}
                    >
                      <X size={12} />
                    </button>
                  </span>
                ))}
                {uploading > 0 && (
                  <span className="assistant-file">
                    <LoaderCircle size={12} className="spin" aria-hidden /> {t("Attaching…")}
                  </span>
                )}
              </div>
            )}
            <div className="assistant-composer">
              <textarea
                ref={input}
                rows={1}
                value={draft}
                aria-label={t("Message the assistant")}
                placeholder={t("Ask about your budget, or drop a file")}
                onChange={(e) => setDraft(e.target.value)}
                onPaste={(e) => {
                  const files = [...e.clipboardData.files];
                  if (files.length) {
                    e.preventDefault();
                    void upload(files);
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    if (canSend) void send(draft);
                  }
                }}
              />
              <div className="assistant-compose-row">
                <button className="icon-button" aria-label={t("Attach a file")} title={t("Attach a statement, receipt or spreadsheet")} onClick={() => fileInput.current?.click()}>
                  <Paperclip size={15} />
                </button>
                <input
                  ref={fileInput}
                  type="file"
                  multiple
                  hidden
                  accept={ACCEPT}
                  onChange={(e) => {
                    void upload([...(e.target.files ?? [])]);
                    e.target.value = "";
                  }}
                />
                <span className="assistant-plan" title={status.model ? t("Model: {model}", { model: status.model }) : undefined}>
                  {planLabel(status)}
                </span>
                {running ? (
                  <button className="assistant-send stop" aria-label={t("Stop")} title={t("Stop")} onClick={() => current.current && void assistant.stop(current.current)}>
                    <Square size={11} fill="currentColor" />
                  </button>
                ) : (
                  <button className="assistant-send" aria-label={t("Send")} title={t("Send (Enter)")} disabled={!canSend} onClick={() => void send(draft)}>
                    <ArrowUp size={15} />
                  </button>
                )}
              </div>
            </div>
          </div>
          {status.provider === "chatgpt" && !status.planNoticeSeen && (
            <div className="assistant-notice-shade">
              <div className="assistant-notice" role="dialog" aria-modal="true" aria-labelledby="assistant-notice-title">
                <h3 id="assistant-notice-title">{t("You're using your ChatGPT plan")}</h3>
                <p>
                  {t("Messages here count toward your ChatGPT plan's usage, the same as using ChatGPT. You can set a limit for Local Budget in ChatGPT's settings.")}
                </p>
                <div className="assistant-notice-actions">
                  <button className="link-button" onClick={() => void assistant.open("usage")}>
                    {t("Manage usage")}
                  </button>
                  <button className="primary" autoFocus onClick={() => void assistant.settings({ planNoticeSeen: true }).then(setStatus)}>
                    {t("Got it")}
                  </button>
                </div>
              </div>
            </div>
          )}
        </>
      )}
      {preview && <ReportPreview threadId={preview.threadId} proposalId={preview.id} onClose={() => setPreview(null)} />}
      {dragging && (
        <div className="assistant-dropzone">
          <Paperclip size={18} aria-hidden /> {t("Drop to attach")}
        </div>
      )}
    </aside>
  );
}
