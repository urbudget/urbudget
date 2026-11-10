import {
  Component,
  Fragment,
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ErrorInfo,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { getVersion } from "@tauri-apps/api/app";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  Copy,
  FileSpreadsheet,
  FileText,
  Flag,
  History as HistoryIcon,
  Image,
  ListChecks,
  LoaderCircle,
  MessageSquareText,
  Paperclip,
  Pencil,
  Square,
  SquarePen,
  ThumbsDown,
  ThumbsUp,
  TriangleAlert,
  X,
} from "lucide-react";
import { api } from "../bridge";
import { Modal } from "../forms";
import { errorText, monthName } from "../format";
import { keyLabel } from "../platform";
import { today, type Budget, type Summary } from "../types";
import { useToast } from "../ui";
import {
  ACCEPT,
  assistant,
  fileData,
  type Attachment,
  type Card,
  type Entry,
  type Proposal,
  type Provider,
  type Revision,
  type Screen,
  type SentWith,
  type Status,
  type ThreadView,
} from "./api";
import { attachable, useComposer } from "./composer";
import { reducedMotion, useFollow } from "./follow";
import { History } from "./History";
import { Markdown } from "./Markdown";
import { describe, logError } from "./errors";
import { ProposalCard, type CardHandlers } from "./ProposalCard";
import { printReport, ReportPreview } from "./Report";
import { ReplyReport, reportDetails } from "./ReplyReport";
import { formatCount, getDirection, getLanguage, msg, msgc, t, tn, useLanguage } from "../i18n";
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
/** The panel's width, and the least the page beside it keeps (`page`). */
export const WIDTH = { min: 320, max: 640, default: 380, page: 420 };
/**
 * The widest the panel may be in a window this wide, with `beside` taken by the sidebar: the
 * page keeps its least width, though the panel is never narrower than its own.
 */
export const widest = (windowWidth: number, beside = 0) => Math.max(WIDTH.min, Math.min(WIDTH.max, windowWidth - beside - WIDTH.page));
/**
 * The panel's width, remembered between sessions. In a window too narrow for it, it is shown
 * narrower than remembered and comes back when the window widens. `beside` is the sidebar's width.
 */
export function useAssistantWidth(beside = 0) {
  const [saved, setSaved] = useState(() => {
    const kept = Number(store.get("lb:assistant:width"));
    return kept >= WIDTH.min && kept <= WIDTH.max ? kept : WIDTH.default;
  });
  const [room, setRoom] = useState(() => window.innerWidth);
  useEffect(() => {
    const resized = () => setRoom(window.innerWidth);
    window.addEventListener("resize", resized);
    return () => window.removeEventListener("resize", resized);
  }, []);
  const most = widest(room, beside);
  const setWidth = (value: number) => {
    const next = Math.round(Math.min(most, Math.max(WIDTH.min, value)));
    setSaved(next);
    store.set("lb:assistant:width", String(next));
  };
  return { width: Math.min(saved, most), setWidth };
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
  import: msg("Import a bank file"),
};

/**
 * Where a message is sent from, in parts: the page's name, the month when that page shows one,
 * and the account or the category in view. Empty when the user took it off the message.
 */
export function place(page: string, month?: string, account?: string, category?: string): string[] {
  if (!page) return [];
  const name = PAGE_NAMES[page];
  return [name ? t(name) : "", page === "budget" && month ? monthName(month) : "", account ?? category ?? ""].filter(Boolean);
}
/** The parts of a place on one line. Each keeps its own direction: an account's name may not be in the app's language. */
function Place({ parts }: { parts: string[] }) {
  return (
    <>
      {parts.map((part, i) => (
        <Fragment key={i}>
          {i > 0 && " · "}
          <bdi>{part}</bdi>
        </Fragment>
      ))}
    </>
  );
}
/** What went with a sent message, small, on its bubble: where it was sent from, the selection and the import. */
function SentWithLine({ sent }: { sent: SentWith }) {
  const parts = place(sent.page, sent.month, sent.account, sent.category);
  const more = [
    sent.transactions > 0 ? tn(sent.transactions, "{count} selected transaction", "{count} selected transactions") : "",
    sent.import ? t("The file being imported") : "",
  ].filter(Boolean);
  if (!parts.length && !more.length) return null;
  return (
    <small className="assistant-sent-with">
      <Place parts={parts} />
      {more.map((text, i) => (
        <Fragment key={i}>
          {(i > 0 || parts.length > 0) && " · "}
          {text}
        </Fragment>
      ))}
    </small>
  );
}

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

/** A reply still arriving is drawn again, and its Markdown read again, this often at most (milliseconds). */
const REVEAL_EVERY = 100;
/**
 * Reveals streamed text at an even pace instead of in network-sized bursts: ten times a second
 * it shows a share of what is waiting, so it keeps up with a fast stream and eases out at the end.
 */
function useReveal(text: string, animate: boolean) {
  const [count, setCount] = useState(() => (animate && !reducedMotion() ? 0 : text.length));
  const shown = useRef(count);
  const target = useRef(text.length);
  target.current = text.length;
  /** When more of it was last shown; kept across pieces of text, which arrive faster than it's drawn. */
  const drawn = useRef(-REVEAL_EVERY);
  useEffect(() => {
    if (shown.current >= text.length) {
      if (shown.current > text.length) setCount((shown.current = text.length));
      return;
    }
    let frame = 0;
    const tick = () => {
      const waiting = target.current - shown.current;
      if (waiting <= 0) return;
      const now = performance.now();
      if (now - drawn.current >= REVEAL_EVERY) {
        drawn.current = now;
        shown.current += Math.max(Math.min(waiting, 2), Math.ceil(waiting * 0.4));
        setCount(shown.current);
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [text]);
  return text.slice(0, count);
}

/**
 * The reply streaming in, kept outside the panel's state: a piece of text arriving draws the
 * reply again and nothing else in the conversation.
 */
function createStream() {
  let text: string | null = null;
  const listeners = new Set<() => void>();
  return {
    get: () => text,
    /** Ending it (null) tells no one: the panel takes the reply out, or makes it an entry, itself. */
    set: (next: string | null) => {
      text = next;
      if (next !== null) for (const listener of listeners) listener();
    },
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
  };
}
type Stream = ReturnType<typeof createStream>;
const unheard = () => () => {};

/** What a report says about the app and the service the reply came from (ReplyReport's details). */
type About = { version: string; provider: Provider; model?: string | null };

/**
 * One reply. A live reply reads its text from the stream and keeps the same element when it
 * finishes, so nothing jumps; its buttons hold their place meanwhile. Copying takes the reply's
 * text as it was written, submitting a report stays inside the app, and a thumb stays on this
 * computer.
 */
const Reply = memo(function Reply({
  text,
  live,
  stream,
  index,
  rating,
  tools,
  about,
  onRate,
}: {
  /** The whole reply, once it is an entry of the conversation. */
  text: string;
  live: boolean;
  stream: Stream;
  /** Its place in the conversation, for rating it; a reply that just arrived has none yet. */
  index?: number;
  rating?: "up" | "down";
  tools?: string[];
  about: About;
  onRate: (index: number, up: boolean | null) => void;
}) {
  const toast = useToast();
  const streamed = useSyncExternalStore(live ? stream.subscribe : unheard, () => (live ? stream.get() : null));
  const whole = live ? (streamed ?? "") : text;
  const [reporting, setReporting] = useState(false);
  const [animate] = useState(live);
  const shown = useReveal(whole, animate);
  const typing = live || shown.length < whole.length;
  // Where there's no clipboard to write to, that is said too, not thrown.
  const copy = () =>
    void Promise.resolve()
      .then(() => navigator.clipboard.writeText(whole))
      .then(
        () => toast({ message: t("Copied.") }),
        (e) => toast({ message: errorText(e), tone: "error" }),
      );
  return (
    <>
      <div className={`assistant-reply${typing ? " typing" : ""}`}>
        <Markdown text={shown} streaming={typing} />
      </div>
      <div className={`assistant-reply-actions${typing ? " waiting" : ""}`}>
        <button className="assistant-flag" tabIndex={typing ? -1 : 0} disabled={typing} onClick={copy}>
          <Copy size={11} aria-hidden />
          {t("Copy")}
        </button>
        <button
          className="assistant-flag"
          title={t("Report this reply to UrBudget's developer")}
          tabIndex={typing ? -1 : 0}
          disabled={typing}
          onClick={() => setReporting(true)}
        >
          <Flag size={11} aria-hidden />
          {t("Report a problem")}
        </button>
        {index !== undefined && (
          <>
            <button
              className="assistant-thumb"
              aria-label={t("Good reply")}
              title={t("Good reply")}
              aria-pressed={rating === "up"}
              disabled={typing}
              onClick={() => onRate(index, rating === "up" ? null : true)}
            >
              <ThumbsUp size={12} aria-hidden />
            </button>
            <button
              className="assistant-thumb"
              aria-label={t("Bad reply")}
              title={t("Bad reply")}
              aria-pressed={rating === "down"}
              disabled={typing}
              onClick={() => onRate(index, rating === "down" ? null : false)}
            >
              <ThumbsDown size={12} aria-hidden />
            </button>
          </>
        )}
      </div>
      {reporting && (
        <ReplyReport text={whole} details={reportDetails({ ...about, tools, language: getLanguage() })} close={() => setReporting(false)} />
      )}
    </>
  );
});

type Step = { text: string; failed: boolean };
function StepLine({ step }: { step: Step }) {
  return (
    <div className={`assistant-step${step.failed ? " failed" : ""}`}>
      {step.failed ? <TriangleAlert size={12} aria-hidden /> : <Check size={12} aria-hidden />}
      <span dir="auto">{step.text}</span>
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
      {/* Opening them is the user's doing: the conversation doesn't scroll for it (see useFollow). */}
      <button className="assistant-steps-toggle" data-grows aria-expanded={open} onClick={() => setOpen(!open)}>
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

/**
 * The latest change still waiting to be applied, whose rows the page highlights. A thing the
 * assistant made isn't one, and neither is a question.
 */
const pending = (entries: Entry[]) =>
  [...entries]
    .reverse()
    .find((e): e is Extract<Entry, { kind: "proposal" }> => e.kind === "proposal" && e.proposal.status === "proposed" && e.proposal.actions.includes("apply"))
    ?.proposal;
/** How long after the window's budget last changed a conversation's waiting cards are brought up to date (milliseconds). */
export const REFRESH = 500;
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

/**
 * What gets sent, and to whom, said once for each service before a first message goes to it.
 * Nothing is sent until the user goes on. The keyboard stays inside it, and Esc is "Not now".
 */
function SentNotice({
  status,
  onStatus,
  onContinue,
  onClose,
}: {
  status: Status;
  onStatus: (s: Status) => void;
  onContinue: () => Promise<void>;
  onClose: () => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const go = async () => {
    if (busy) return;
    setBusy(true);
    try {
      await onContinue();
    } finally {
      setBusy(false);
    }
  };
  return (
    // A click beside the notice leaves the keyboard where it is, inside it.
    <div className="assistant-notice-shade" onMouseDown={(e) => e.target === e.currentTarget && e.preventDefault()}>
      <div
        className="assistant-notice"
        role="dialog"
        aria-modal="true"
        aria-labelledby="assistant-sent-title"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onClose();
          } else if (e.key === "Enter" && e.repeat) {
            // Enter still held down from sending the message isn't an answer.
            e.preventDefault();
          } else if (e.key === "Tab") {
            const stops = [...e.currentTarget.querySelectorAll<HTMLElement>("button, input")];
            if (document.activeElement !== stops[e.shiftKey ? 0 : stops.length - 1]) return;
            e.preventDefault();
            stops[e.shiftKey ? stops.length - 1 : 0].focus();
          }
        }}
      >
        <h3 id="assistant-sent-title">{t("What gets sent")}</h3>
        <p>
          {t("Your messages, what's on screen, the files you attach, and the budget details the assistant looks up to answer you go to {provider}. Its terms and privacy policy govern how it processes and retains them.", {
            provider: status.provider === "anthropic" ? "Anthropic" : "OpenAI",
          })}
        </p>
        <p>{t("Your budget file, conversation history and sign-in are kept on this computer. Nothing in the budget changes until you apply a change yourself.")}</p>
        <label className="setting-row toggle">
          <span>
            <strong>{t("Share memos")}</strong>
            <small>{t("Dates, payees, amounts and categories are always shared when the assistant looks something up.")}</small>
          </span>
          <input
            type="checkbox"
            role="switch"
            checked={status.shareMemos}
            onChange={(e) =>
              void assistant
                .settings({ shareMemos: e.target.checked })
                .then(onStatus)
                .catch((err) => toast({ message: errorText(err), tone: "error" }))
            }
          />
        </label>
        <div className="assistant-notice-actions">
          <button className="link-button" onClick={onClose}>
            {t("Not now")}
          </button>
          <button className="primary" autoFocus onClick={() => void go()}>
            {t("Continue")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The same functions for a component's whole life, each calling the one given last. They go to
 * memoised children (cards, replies), which then aren't drawn again just because the panel was.
 */
function useLatest<T extends Record<string, (...args: never[]) => unknown>>(now: T): T {
  const latest = useRef(now);
  latest.current = now;
  return useMemo(() => {
    const steady: Record<string, unknown> = {};
    for (const name of Object.keys(now)) steady[name] = (...args: never[]) => latest.current[name](...args);
    return steady as T;
  }, []);
}

/** What a card's buttons do, by the conversation and the proposal they belong to. */
type CardActions = {
  apply: (threadId: string, p: Proposal) => Promise<void>;
  revise: (threadId: string, p: Proposal, revision: Revision) => Promise<Proposal>;
  discard: (threadId: string, p: Proposal) => Promise<void>;
  restore: (threadId: string, p: Proposal) => Promise<void>;
  undo: (threadId: string, p: Proposal) => Promise<void>;
  askChanges: (threadId: string, p: Proposal) => void;
  askAgain: (threadId: string, p: Proposal) => void;
  show: (threadId: string, p: Proposal) => void;
  answer: (threadId: string, p: Proposal, label: string) => void;
  review: (threadId: string, p: Proposal) => void;
  preview: (threadId: string, p: Proposal) => void;
  print: (threadId: string, p: Proposal) => Promise<void>;
  sheet: (threadId: string, p: Proposal, open: boolean) => Promise<void>;
  save: (threadId: string, p: Proposal) => Promise<void>;
};

/**
 * A proposal's card in the conversation, drawn again only when what it's given changes: the
 * proposal, the budget, or whether its change can still be undone, its fixes applied or its
 * question answered.
 */
const ProposalItem = memo(function ProposalItem({
  threadId,
  proposal: p,
  budget,
  budgetId,
  canUndo,
  fixable,
  answerable,
  does,
}: {
  threadId: string;
  proposal: Proposal;
  budget: Budget;
  budgetId: string;
  canUndo: boolean;
  fixable: boolean;
  answerable: boolean;
  does: CardActions;
}) {
  const on = useMemo<CardHandlers>(
    () => ({
      // The card as the core just made it with the user's choices, when there is one: an
      // import's fixes are applied from the card itself.
      apply: (now) => does.apply(threadId, now ?? p),
      revise: (revision) => does.revise(threadId, p, revision),
      discard: () => does.discard(threadId, p),
      restore: () => does.restore(threadId, p),
      undo: () => does.undo(threadId, p),
      askChanges: () => does.askChanges(threadId, p),
      askAgain: () => does.askAgain(threadId, p),
      show: () => does.show(threadId, p),
      answer: (label) => does.answer(threadId, p, label),
      review: () => does.review(threadId, p),
      saveCsv: () => does.save(threadId, p),
      preview: () => does.preview(threadId, p),
      print: () => does.print(threadId, p),
      sheet: (open) => does.sheet(threadId, p, open),
    }),
    [does, threadId, p],
  );
  return <ProposalCard budgetId={budgetId} proposal={p} budget={budget} canUndo={canUndo} fixable={fixable} answerable={answerable} on={on} />;
});

export function AssistantPanel({
  budget,
  budgetId,
  summary,
  screen,
  request,
  onRequestTaken,
  width,
  onResize,
  onClose,
  onOpenSettings,
  onApply,
  onUndo,
  canUndo,
  canApplyFix,
  onReviewImport,
  onHighlight,
  onShow,
  onWorking,
  autoFocus = false,
}: {
  budget: Budget;
  budgetId: string;
  summary: Summary;
  screen: Omit<Screen, "selected" | "today">;
  /** The user just opened the panel (its button, Ctrl+J): the keyboard goes to the message field. */
  autoFocus?: boolean;
  request: Request | null;
  /** The request is in the message box now: whoever made it forgets it, so reopening the panel doesn't bring it back. */
  onRequestTaken?: () => void;
  width: number;
  onResize: (width: number) => void;
  onClose: () => void;
  onOpenSettings: () => void;
  /** Applies a proposal to the budget; resolves to the card as it is afterwards. */
  onApply: (threadId: string, p: Proposal) => Promise<Proposal>;
  /** Takes an applied proposal's change back; fails when the window's Undo didn't. */
  onUndo: (threadId: string, p: Proposal) => Promise<Proposal>;
  /** The window can still take the proposal's change back: it is the budget's latest. */
  canUndo: (p: Proposal) => boolean;
  /** The import a card's fixes are for is open. Left out, any is taken to be. */
  canApplyFix?: (p: Proposal) => boolean;
  onReviewImport: (threadId: string, p: Proposal) => void;
  onHighlight: (highlight: Card["highlight"] | null) => void;
  /** Goes to what a card names: its transactions in the register, or its categories on the Budget page. */
  onShow?: (highlight: Card["highlight"]) => void;
  /** Whether there's a message being written, attached to or sent, which closing the app would lose. */
  onWorking?: (working: boolean) => void;
}) {
  const toast = useToast();
  const threadKey = `lb:assistant:thread:${budgetId}`;
  const [status, setStatus] = useState<Status | null>(null);
  /** The status couldn't be read: the panel says so, with a way to try again. */
  const [unreachable, setUnreachable] = useState(false);
  const [threadId, setThreadId] = useState<string | null>(() => store.get(threadKey));
  /**
   * A conversation being opened: on its way, or it couldn't be read. Null once it is there, and
   * when there is none. Until it's known, nothing is offered that would send into it.
   */
  const [opening, setOpening] = useState<"loading" | "failed" | null>(() => (store.get(threadKey) ? "loading" : null));
  const [entries, setEntries] = useState<Entry[]>([]);
  /** The open conversation besides what was said in it: its title, what it has used, whether it has grown long, what it carries on, and what's waiting in the budget to offer under its last reply. */
  const [meta, setMeta] = useState<Pick<ThreadView, "title" | "usage" | "long" | "continuedFrom" | "next"> | null>(null);
  /** Entries from this index on arrived while the panel was open, and fade in. */
  const [freshFrom, setFreshFrom] = useState(0);
  const [running, setRunning] = useState(false);
  /** The reply streaming in right now, before it is complete: its text is in `stream`, and here only whether there is one. */
  const [stream] = useState(createStream);
  const [streaming, setStreaming] = useState(false);
  const setLive = (text: string | null) => {
    stream.set(text);
    setStreaming(text !== null);
  };
  /** The model's reasoning summary so far, and the heading or sentence of it that shows while it works. */
  const thought = useRef("");
  const [thinking, setThinkingTitle] = useState("");
  const setThinking = (text: string) => {
    thought.current = text;
    setThinkingTitle(thinkingTitle(text));
  };
  /** What it is doing now (a lookup under way), as the core words it. */
  const [activity, setActivity] = useState("");
  /** What is read aloud: a reply that finished, a card that arrived. Never the text as it streams. */
  const [said, setSaid] = useState("");
  const replied = useRef(false);
  // What's in the message box is kept outside the panel, so it's still there when the panel reopens.
  const [{ draft, attachments, selected, selectedOf }, compose] = useComposer(budgetId);
  const setDraft = (text: string) => compose((c) => ({ ...c, draft: text }));
  /** What the user took off the next message: where they are, the file being imported. It all goes with the one after. */
  const [without, setWithout] = useState({ place: false, import: false });
  const [uploading, setUploading] = useState(0);
  /** What waits behind the notice of what gets sent: a message, or asking again after an error. */
  const [held, setHeld] = useState<{ text: string } | "retry" | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const [problem, setProblem] = useState("");
  const [historyOpen, setHistoryOpen] = useState(false);
  /** The conversation's title as it is being retyped, or null. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [carrying, setCarrying] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [preview, setPreview] = useState<{ threadId: string; id: string } | null>(null);
  /** The app's version, for a report's details; empty when it can't be read. */
  const [version, setVersion] = useState("");
  const current = useRef(threadId);
  current.current = threadId;
  /** The budget as the window has it now: a conversation's waiting cards are brought up to date with it. */
  const budgetNow = useRef(budget);
  budgetNow.current = budget;
  const historyButton = useRef<HTMLButtonElement>(null);
  const renameButton = useRef<HTMLButtonElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const body = useRef<HTMLDivElement>(null);
  const feed = useRef<HTMLDivElement>(null);
  const handled = useRef<number | null>(null);
  /** The conversation goes to its end when it is next drawn: it was just opened, or a message was just sent. */
  const landing = useRef(true);

  const remember = (id: string | null) => {
    setThreadId(id);
    current.current = id;
    store.set(threadKey, id);
  };
  /** Loads a conversation; `opening` means none of it is new, so nothing fades in, and it lands at its end. */
  const load = useCallback(async (id: string | null, opening = false) => {
    if (!id) {
      setEntries([]);
      setMeta(null);
      setOpening(null);
      setFreshFrom(0);
      setRunning(false);
      setLive(null);
      return;
    }
    if (opening) {
      // Nothing of the conversation left behind shows while this one is on its way.
      setOpening("loading");
      setEntries([]);
      setMeta(null);
    }
    try {
      const view = await assistant.thread(budgetNow.current, id);
      if (current.current !== id) return;
      setEntries((prev) => (same(prev, view.entries) ? prev : view.entries));
      setMeta({ title: view.title, usage: view.usage, long: view.long, continuedFrom: view.continuedFrom, next: view.next });
      setOpening(null);
      if (opening) {
        setFreshFrom(view.entries.length);
        landing.current = true;
      }
      setRunning(view.running);
      if (!view.running) {
        setLive(null);
        setThinking("");
        setActivity("");
      }
    } catch {
      // It says so instead of forgetting the conversation: one that can't be read now may read later.
      if (opening && current.current === id) setOpening("failed");
    }
  }, []);

  const readStatus = useCallback(() => {
    setUnreachable(false);
    void assistant.status().then(setStatus, () => setUnreachable(true));
  }, []);
  useEffect(() => {
    readStatus();
    const refresh = () => void assistant.status().then(setStatus).catch(() => {});
    window.addEventListener("focus", refresh);
    // A service chosen in Settings while the panel is open is the one a message goes to, and
    // the one the notice of what gets sent is about.
    const unwatch = assistant.onStatus(setStatus);
    // Where there's no native side to ask (a browser), reports go without the version.
    void Promise.resolve().then(getVersion).then(setVersion, () => {});
    return () => {
      window.removeEventListener("focus", refresh);
      unwatch();
    };
  }, []);
  // Another budget has its own conversations, and nothing of the last one's waiting to be sent.
  useEffect(() => {
    const id = store.get(threadKey);
    remember(id);
    setHeld(null);
    setRenaming(null);
    setWithout({ place: false, import: false });
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
            setActivity("");
            setSaid("");
            replied.current = false;
            break;
          case "thinking":
            setThinking(thought.current + e.text);
            break;
          case "activity":
            setActivity(e.text);
            break;
          case "delta":
            setActivity("");
            setLive((stream.get() ?? "") + e.text);
            break;
          case "message":
            setLive(null);
            setThinking("");
            setActivity("");
            replied.current = true;
            // Its place comes with it, so its thumbs are there as soon as it has finished.
            setEntries((list) => [...list, { kind: "assistant", text: e.text, index: e.index }]);
            break;
          case "step": {
            // A reply cut short by a lookup ends here, keeping its place and element.
            const text = stream.get();
            setLive(null);
            setThinking("");
            setActivity("");
            if (text) replied.current = true;
            setEntries((list) => [...list, ...(text ? [{ kind: "assistant" as const, text }] : []), { kind: "step", text: e.text, failed: e.failed }]);
            break;
          }
          case "proposal":
            setEntries((list) => [...list, { kind: "proposal", proposal: e.proposal }]);
            setSaid(t("A proposal is ready to review."));
            break;
          case "replaced":
            // The earlier card as it is now that a newer one corrects it: in its place, not after it.
            setEntries((list) => list.map((entry) => (entry.kind === "proposal" && entry.proposal.id === e.proposal.id ? { kind: "proposal", proposal: e.proposal } : entry)));
            break;
          case "error":
            setLive(null);
            setThinking("");
            setActivity("");
            setEntries((list) => [...list, { kind: "error", text: e.text, code: e.code, action: e.action }]);
            break;
          case "cut":
            setEntries((list) => [...list, { kind: "cut" }]);
            break;
          case "done":
            setRunning(false);
            setThinking("");
            setActivity("");
            // Said once, when the whole of it is there: never the text as it streams.
            if (replied.current) setSaid(t("The assistant replied."));
            replied.current = false;
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

  // Not set up, with nothing to show: the sign-in screen stands in for the conversation.
  const settingUp = !!status && !status.ready && opening !== "loading" && entries.length === 0;
  /** The conversation is on screen, with the message field under it. */
  const conversing = !!status && !settingUp;
  // One rule for scrolling: new text is followed only while the user is at the end (useFollow).
  const scroll = useFollow(body, feed, conversing);
  useLayoutEffect(() => {
    if (!body.current) return;
    if (!landing.current) return scroll.follow();
    // A conversation still on its way has no end to go to yet.
    if (opening === "loading") return;
    landing.current = false;
    scroll.land();
  }, [entries, streaming, running, opening, conversing]);

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
  // A card that changes outside the panel (the window's Undo took its change back, its import
  // was finished) is the one the app marked: it takes its place here as it is now.
  useEffect(
    () =>
      assistant.onMarked((id, p) => {
        if (id === current.current) replace(p);
      }),
    [],
  );
  // The core brings waiting changes up to date whenever the conversation is read. So it is read
  // again once the budget has stopped changing, while a change waits here (a card that was
  // updated says so), or while it ends on a reply: the next steps offered under one are counted
  // from the budget too. Not while a reply is arriving: that ends by reading it anyway.
  const stale = useRef(false);
  stale.current =
    !running &&
    !opening &&
    (entries[entries.length - 1]?.kind === "assistant" || entries.some((e) => e.kind === "proposal" && e.proposal.actions.includes("apply")));
  const settled = useRef(budget);
  useEffect(() => {
    if (settled.current === budget) return;
    settled.current = budget;
    const timer = setTimeout(() => {
      if (stale.current && current.current) void load(current.current);
    }, REFRESH);
    return () => clearTimeout(timer);
  }, [budget]);

  const upload = async (files: File[]) => {
    const room = 5 - attachments.length;
    if (files.length > room) toast({ message: t("Attach up to 5 files at a time."), tone: "error" });
    for (const file of files.slice(0, Math.max(0, room))) {
      setUploading((n) => n + 1);
      try {
        const ready = await fileData(file);
        const saved = await assistant.attach(ready.name, ready.data);
        compose((c) => ({ ...c, attachments: [...c.attachments, saved] }));
      } catch (e) {
        toast({ message: errorText(e), tone: "error" });
      } finally {
        setUploading((n) => n - 1);
      }
    }
    input.current?.focus();
  };

  // What goes with the next message, as the line above the field says it: where the user is,
  // the transactions they selected, and the bank file being imported. A part taken off isn't sent.
  const account = screen.accountId ? budget.accounts.find((a) => a.id === screen.accountId)?.name : undefined;
  const category = screen.categoryId ? budget.categories.find((c) => c.id === screen.categoryId)?.name : undefined;
  const where = without.place ? [] : place(screen.page, screen.month, account, category);
  const importing = !without.import && !!screen.import;

  const send = async (text: string) => {
    // Nothing goes into a conversation that is still on its way, or that couldn't be read.
    if (!status?.ready || running || uploading || opening) return;
    const message = text.trim();
    if (!message && !attachments.length) return;
    setProblem("");
    // A message being written that isn't this one (a card's or a next step's button sent its own
    // words) stays in the field.
    const writing = draft.trim() && draft.trim() !== message ? draft : "";
    const sent = { draft: writing || message, attachments, selected, selectedOf };
    const from = without.place
      ? { page: "" }
      : { page: screen.page, month: screen.month, accountId: screen.accountId, categoryId: screen.categoryId };
    const file = importing ? screen.import : undefined;
    landing.current = true;
    setEntries((list) => [
      ...list,
      {
        kind: "user",
        text: message,
        attachments,
        with: {
          page: from.page,
          month: without.place ? "" : (screen.month ?? ""),
          account: without.place ? undefined : account,
          category: without.place ? undefined : category,
          transactions: selected.length,
          import: !!file,
        },
      },
    ]);
    compose(() => ({ draft: writing, attachments: [], selected: [] }));
    setRunning(true);
    setLive(null);
    setThinking("");
    setActivity("");
    try {
      const id = await assistant.send(budget, current.current, message, sent.attachments.map((a) => a.id), {
        ...from,
        selected: sent.selected,
        today: today(),
        import: file,
      });
      // The next message starts with all of it again.
      setWithout({ place: false, import: false });
      if (id !== current.current) remember(id);
      await load(id);
    } catch (e) {
      setRunning(false);
      setEntries((list) => list.slice(0, -1));
      compose(() => sent);
      setProblem(errorText(e));
    }
  };
  /** Sends a message, once the user has been told what gets sent and to whom: the notice asks first. */
  const submit = (text: string) => {
    if (!status?.ready || running || uploading || opening) return;
    if (!text.trim() && !attachments.length) return;
    if (status.noticeDue) setHeld({ text });
    else void send(text);
  };

  // Entry points: "Ask about these", a dropped file, a question from the inspector.
  useEffect(() => {
    // Not before it's known what the open conversation holds.
    if (!request || handled.current === request.key || !status || opening === "loading") return;
    handled.current = request.key;
    onRequestTaken?.();
    // Something asked from elsewhere is its own subject: it starts a conversation, unless a
    // reply is still arriving in this one (then it waits in the message field as it is).
    const fresh = (entries.length > 0 || opening === "failed") && !running;
    if (fresh) newConversation();
    const { text, selected: chosen } = request;
    // Only as many as a message takes; the field says how many were left out.
    if (chosen?.length) compose((c) => ({ ...c, ...attachable(chosen) }));
    if (request.files?.length) void upload(request.files);
    // A message already being written is kept: the request goes on a new line after it.
    if (text && request.send && status.ready && !draft.trim() && !opening) submit(text);
    else if (text) compose((c) => ({ ...c, draft: c.draft.trim() ? `${c.draft.trimEnd()}\n${text}` : text }));
    setTimeout(() => input.current?.focus());
  }, [request, status, opening]);
  // Opened by the user: the keyboard goes to the message field, once, when the field is there
  // and no notice is in front of it.
  const focused = useRef(false);
  /** The plan's notice is up: it comes before the notice of what gets sent. */
  const planNotice = !!status?.ready && status.provider === "chatgpt" && !status.planNoticeSeen;
  const noticed = planNotice || !!held;
  const readPlanNotice = () => void assistant.settings({ planNoticeSeen: true }).then(setStatus);
  useEffect(() => {
    if (!autoFocus || focused.current || noticed || !input.current) return;
    focused.current = true;
    input.current.focus();
  }, [autoFocus, conversing, noticed]);
  // The message field is as tall as its text (up to 160px) however the text got there: typed,
  // filled in by a request from elsewhere (the import's "Fix with the assistant"), or put back
  // after a send that failed. It appears once the assistant is ready, with any text already in it.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [draft, status?.ready]);

  /** What's on screen of the conversation being left, cleared for the next one. */
  const leave = () => {
    setLive(null);
    setThinking("");
    setActivity("");
    setProblem("");
    setRenaming(null);
  };
  /**
   * Starts a conversation. Like opening another one, it can be done while a reply is arriving:
   * that reply goes on in the conversation left behind, and is there when it's opened again.
   */
  const newConversation = () => {
    remember(null);
    leave();
    setEntries([]);
    setMeta(null);
    setOpening(null);
    setFreshFrom(0);
    setRunning(false);
    setTimeout(() => input.current?.focus());
  };
  /** Opens a conversation: one from the history, or the one that carries this one on. */
  const open = (id: string) => {
    remember(id);
    leave();
    void load(id, true);
  };
  /** Carries the conversation on in a new one, which begins with what was said last in this one. */
  const carryOn = async () => {
    const from = current.current;
    if (!from || carrying) return;
    setCarrying(true);
    try {
      const id = await assistant.continue(from);
      if (current.current === from) open(id);
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    } finally {
      setCarrying(false);
    }
  };
  /** Keeps the title typed in the header. Empty, it isn't one. */
  const saveTitle = async () => {
    const id = current.current;
    const title = (renaming ?? "").trim();
    if (!id || !title) return;
    if (title === meta?.title) return setRenaming(null);
    try {
      await assistant.renameThread(id, title);
      if (current.current === id) setMeta((was) => (was ? { ...was, title } : was));
      setRenaming(null);
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    }
  };
  // The keyboard goes back to the pencil that opened the title's field, unless the field closed
  // because the user went somewhere else.
  const wasRenaming = useRef(false);
  useEffect(() => {
    const nowhere = !document.activeElement || document.activeElement === document.body;
    if (wasRenaming.current && renaming === null && nowhere) renameButton.current?.focus();
    wasRenaming.current = renaming !== null;
  }, [renaming === null]);
  /** The user's thumb on a reply: up, down, or off again (null). */
  const rate = async (index: number, up: boolean | null) => {
    const id = current.current;
    if (!id) return;
    try {
      await assistant.rate(id, index, up);
      if (current.current !== id) return;
      const rating = up === null ? undefined : up ? ("up" as const) : ("down" as const);
      setEntries((list) => list.map((e) => (e.kind === "assistant" && e.index === index ? { ...e, rating } : e)));
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
    }
  };
  const replace = (p: Proposal) =>
    setEntries((list) => list.map((e) => (e.kind === "proposal" && e.proposal.id === p.id ? { kind: "proposal", proposal: p } : e)));
  /**
   * What a card's button does, and the card as it is afterwards. When it can't be done (the
   * proposal is gone, or was handled somewhere else), that is said and the conversation is read
   * again, so the card shows what the core has and not a button that no longer works.
   */
  const act = <A extends unknown[]>(work: (...args: A) => Promise<Proposal>) => async (...args: A) => {
    try {
      const now = await work(...args);
      replace(now);
      return now;
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
      if (current.current) void load(current.current);
      throw e;
    }
  };
  /** The same, for a button that only needs to know when it's done. */
  const done = <A extends unknown[]>(work: (...args: A) => Promise<Proposal>) => {
    const acted = act(work);
    return async (...args: A) => {
      await acted(...args);
    };
  };
  /** Words for the message field, added after whatever is being written, with the keyboard at their end. */
  const write = (text: string) => {
    compose((c) => ({ ...c, draft: c.draft.trim() ? `${c.draft.trimEnd()}\n${text}` : text }));
    setTimeout(() => {
      const field = input.current;
      if (!field) return;
      field.focus();
      field.setSelectionRange(field.value.length, field.value.length);
    });
  };
  /** A message can be sent now: the assistant is ready, and nothing is arriving, attaching or still opening. */
  const sendable = !!status?.ready && !running && !uploading && !opening;
  /** A message a button sends for the user. When one can't be sent right now, it waits in the field. */
  const say = (text: string) => {
    if (sendable) submit(text);
    else write(text);
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
  /** Asks again after an error. That sends the conversation too, so the notice comes first when it's due. */
  const askAgain = (now: Status | null = status) => {
    if (now?.ready && now.noticeDue) setHeld("retry");
    else void retry();
  };
  /**
   * Signs in with ChatGPT again. From an error that asked for it, the question is then asked
   * again by itself (`ask`); from the banner over the message field, signing in is all.
   */
  const signInAgain = async (ask: boolean) => {
    setSigningIn(true);
    try {
      const next = await assistant.signIn();
      setStatus(next);
      if (ask && next.ready) askAgain(next);
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
      void assistant.status().then(setStatus).catch(() => {});
    } finally {
      setSigningIn(false);
    }
  };
  const closeNotice = () => {
    setHeld(null);
    setTimeout(() => input.current?.focus());
  };
  /** The user read the notice and goes on: it's marked read for this service, and what waited is sent. */
  const proceed = async () => {
    const waited = held;
    if (!waited) return;
    try {
      setStatus(await assistant.settings({ told: true }));
    } catch (e) {
      toast({ message: errorText(e), tone: "error" });
      return;
    }
    closeNotice();
    if (waited === "retry") void retry();
    else void send(waited.text);
  };

  // What cards and replies do. They're handed the same functions for the panel's whole life
  // (useLatest), so typing a message or a reply streaming in doesn't draw each of them again.
  const does = useLatest({
    apply: done((id: string, p: Proposal) => onApply(id, p)),
    revise: act((id: string, p: Proposal, revision: Revision) => assistant.revise(budget, id, p.id, revision)),
    // A thing the assistant made is removed the same way a change is set aside.
    discard: done((id: string, p: Proposal) => assistant.mark(id, p.id, "discarded")),
    restore: done((id: string, p: Proposal) => assistant.mark(id, p.id, "proposed")),
    undo: done((id: string, p: Proposal) => onUndo(id, p)),
    askChanges: (_id: string, p: Proposal) => write(t("Revise the proposal “{title}”: ", { title: p.title })),
    askAgain: (_id: string, p: Proposal) =>
      say(t("Prepare “{title}” again. It couldn't be applied: {reason}", { title: p.title, reason: p.message?.text ?? "" })),
    show: (_id: string, p: Proposal) => onShow?.(p.card.highlight),
    // Through the same door as Send, so the notice of what gets sent and what goes with the message apply.
    answer: (_id: string, _p: Proposal, label: string) => submit(label),
    review: (id: string, p: Proposal) => onReviewImport(id, p),
    preview: (id: string, p: Proposal) => setPreview({ threadId: id, id: p.id }),
    print: async (id: string, p: Proposal) => {
      try {
        await printReport(await assistant.report(id, p.id));
      } catch (err) {
        toast({ message: errorText(err), tone: "error" });
      }
    },
    sheet: async (id: string, p: Proposal, open: boolean) => {
      try {
        const path = await assistant.spreadsheet(budget, id, p.id, open);
        if (open || !path) return;
        const name = path.split(/[\\/]/).pop() ?? path;
        const report = p.kind === "report";
        toast({
          message: t("Saved “{name}”.", { name }),
          action: report
            ? { label: t("Open"), onClick: () => void assistant.spreadsheet(budget, id, p.id, true).catch((e) => toast({ message: errorText(e), tone: "error" })) }
            : { label: t("Show in folder"), onClick: () => void api.reveal(path).catch((e) => toast({ message: errorText(e), tone: "error" })) },
        });
      } catch (err) {
        toast({ message: errorText(err), tone: "error" });
      }
    },
    save: async (id: string, p: Proposal) => {
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
    },
    rate,
  });
  /** What a report of a reply says about where the reply came from. */
  const about = useMemo<About>(
    () => ({ version, provider: status?.provider ?? "chatgpt", model: status?.model }),
    [version, status?.provider, status?.model],
  );

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
            {e.text && <p dir="auto">{e.text}</p>}
            {e.with && <SentWithLine sent={e.with} />}
          </div>
        );
      case "assistant":
        return <Reply text={e.text} live={false} stream={stream} index={e.index} rating={e.rating} tools={e.tools} about={about} onRate={does.rate} />;
      case "step":
        return null;
      case "proposal":
        return (
          <ProposalItem
            threadId={current.current!}
            proposal={e.proposal}
            budget={budget}
            budgetId={budgetId}
            canUndo={canUndo(e.proposal)}
            fixable={e.proposal.kind !== "fix" || (canApplyFix?.(e.proposal) ?? true)}
            // Only a question waits on whether a message can be sent: the other cards aren't drawn again for it.
            answerable={e.proposal.kind !== "question" || sendable}
            does={does}
          />
        );
      case "error": {
        // The core says what helps; only the error the conversation ends on offers it.
        const action = last && !running ? (e.action ?? "none") : "none";
        return (
          <div className="assistant-problem" role="alert">
            <strong>{e.code === "limit" ? t("Usage limit reached") : e.code === "refused" ? t("Declined") : t("Something went wrong")}</strong>
            <p>{e.text}</p>
            {action !== "none" && (
              <div className="assistant-problem-actions">
                {action === "usage" ? (
                  <button className="primary small" onClick={() => void assistant.open("usage")}>
                    {t("Manage usage")}
                  </button>
                ) : action === "signIn" ? (
                  <>
                    <button className="primary small" disabled={signingIn} aria-busy={signingIn} onClick={() => void signInAgain(true)}>
                      {signingIn && <LoaderCircle size={13} className="spin" aria-hidden />} {t("Continue with ChatGPT")}
                    </button>
                    {signingIn && (
                      <button className="link-button" onClick={() => void assistant.cancelSignIn()}>
                        {t("Cancel")}
                      </button>
                    )}
                  </>
                ) : action === "settings" ? (
                  <button className="primary small" onClick={onOpenSettings}>
                    {t("Open settings")}
                  </button>
                ) : action === "newConversation" ? (
                  <button className="primary small" disabled={carrying} onClick={() => void carryOn()}>
                    {t("Continue in a new conversation")}
                  </button>
                ) : (
                  <button className="secondary small" onClick={() => askAgain()}>
                    {t("Try again")}
                  </button>
                )}
              </div>
            )}
          </div>
        );
      }
      case "cut":
        // Asking again is how its rest is asked for, and only where the conversation ends.
        return (
          <div className="assistant-cut">
            <span>{t("The reply was cut short.")}</span>
            {last && !running && (
              <button className="secondary small" onClick={() => askAgain()}>
                {t("Continue")}
              </button>
            )}
          </div>
        );
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
  if (streaming)
    feedItems.push(
      <div key={entries.length} className="assistant-item enter">
        <Reply text="" live stream={stream} about={about} onRate={does.rate} />
      </div>,
    );
  else if (running)
    // While no text is arriving: what it is doing now, or else what it is thinking about.
    feedItems.push(
      <div key="activity" className="assistant-item assistant-activity enter">
        <span className="assistant-dots" aria-hidden>
          <i />
          <i />
          <i />
        </span>
        <span className="assistant-activity-text">{activity ? `${activity}…` : thinking || t("Thinking")}</span>
      </div>,
    );
  else if (meta?.next?.length && entries.length > 0)
    // Under the last reply, once nothing is arriving: what's waiting in the budget that the
    // assistant can help with. The core words them, in the app's language.
    feedItems.push(
      <div key="next" className="assistant-item assistant-next">
        {meta.next.map((step) => (
          <button key={step.id} className="assistant-idea" disabled={!sendable} onClick={() => submit(step.prompt)}>
            {step.text}
          </button>
        ))}
      </div>,
    );

  const language = useLanguage();
  const ideas = useMemo(() => suggestions(budget, summary, screen), [budget, summary, screen.page, screen.month, screen.accountId, language]);
  const canSend = !!status?.ready && !running && !uploading && !opening && (!!draft.trim() || attachments.length > 0);
  const last = entries[entries.length - 1];
  // Not ready any more (signed out, the key removed) in a conversation that has messages: the
  // one thing that helps is offered over the field, unless the conversation's last error offers it.
  const notReady = !!status && !status.ready && !(last?.kind === "error" && (last.action === "signIn" || last.action === "settings"));
  /** A selection longer than a message takes: said by the field, so nothing is left out unseen. */
  const capped = selectedOf ? t("Only the first 200 of {n} selected transactions are attached.", { n: formatCount(selectedOf) }) : "";

  return (
    <aside
      className={`assistant${dragging ? " dragging" : ""}`}
      aria-label={t("Assistant")}
      onKeyDown={(e) => {
        // Esc closes the panel, after whatever is open in it. A menu, the title's field and a
        // card's editor each take it themselves, and so does a dialog of the panel's, which is
        // drawn outside it; a notice is closed here. While a reply is arriving it does nothing:
        // stopping one is a button.
        if (e.key !== "Escape" || e.defaultPrevented || e.nativeEvent.isComposing) return;
        if (!e.currentTarget.contains(e.target as Node)) return;
        if (!noticed && running) return;
        e.stopPropagation();
        if (planNotice) readPlanNotice();
        else if (held) closeNotice();
        else onClose();
      }}
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
        {/* The open conversation's title, which its pencil turns into a field: Enter keeps what's typed, Esc leaves it. */}
        {renaming !== null ? (
          <form
            className="assistant-rename"
            onSubmit={(e) => {
              e.preventDefault();
              void saveTitle();
            }}
            onBlur={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setRenaming(null);
            }}
          >
            <input
              autoFocus
              aria-label={t("Conversation name")}
              maxLength={80}
              value={renaming}
              onFocus={(e) => e.target.select()}
              onChange={(e) => setRenaming(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== "Escape") return;
                e.stopPropagation();
                setRenaming(null);
              }}
            />
            {/* Pressing it leaves the keyboard in the field: where a click doesn't move it to the button, the field would close first. */}
            <button className="icon-button" aria-label={t("Save")} title={t("Save")} disabled={!renaming.trim()} onMouseDown={(e) => e.preventDefault()}>
              <Check size={15} />
            </button>
          </form>
        ) : threadId && meta?.title ? (
          <>
            <strong className="assistant-title" dir="auto" title={meta.title}>
              {meta.title}
            </strong>
            <button
              ref={renameButton}
              className="icon-button assistant-rename-open"
              aria-label={t("Rename conversation")}
              title={t("Rename conversation")}
              onClick={() => setRenaming(meta.title)}
            >
              <Pencil size={13} />
            </button>
          </>
        ) : (
          <strong>{t("Assistant")}</strong>
        )}
        <div className="assistant-tools">
          {conversing && (
            <>
              <button ref={historyButton} className="icon-button" aria-label={t("Conversations")} title={t("Conversations")} onClick={() => setHistoryOpen(true)}>
                <HistoryIcon size={15} />
              </button>
              <button className="icon-button" aria-label={t("New conversation")} title={t("New conversation")} onClick={newConversation}>
                <SquarePen size={15} />
              </button>
            </>
          )}
          <button className="icon-button" aria-label={t("Close the assistant")} title={keyLabel(t("Close (Ctrl+J)"))} onClick={onClose}>
            <X size={16} />
          </button>
        </div>
        <History
          open={historyOpen}
          onClose={() => setHistoryOpen(false)}
          anchor={historyButton}
          current={threadId}
          usage={meta?.usage}
          running={running}
          onOpen={open}
          onDelete={() => setDeleting(true)}
        />
      </header>
      {/* Read aloud once: a reply that finished, a card that arrived. The conversation itself isn't a live region, so text isn't read as it streams. */}
      <div className="sr-only" aria-live="polite" aria-atomic="true">
        {said}
      </div>

      {!status ? (
        unreachable ? (
          <div className="assistant-loading" role="alert">
            <p>{t("The assistant couldn't be loaded.")}</p>
            <button className="secondary small" onClick={readStatus}>
              {t("Try again")}
            </button>
          </div>
        ) : (
          <div className="assistant-loading" role="status">
            <LoaderCircle size={15} className="spin" aria-hidden /> {t("Checking the assistant…")}
          </div>
        )
      ) : settingUp ? (
        <SignIn status={status} onStatus={setStatus} onOpenSettings={onOpenSettings} />
      ) : (
        <>
          <div className="assistant-body" ref={body} onScroll={scroll.onScroll} onWheel={scroll.onWheel}>
            {/* A click on something that opens in place (data-grows) makes the conversation taller by the user's own doing. */}
            <div
              className="assistant-feed"
              ref={feed}
              onClickCapture={(e) => {
                if ((e.target as Element).closest?.("[data-grows]")) scroll.own();
              }}
            >
              {meta?.continuedFrom && <div className="assistant-continued">{t("Continued from “{title}”", { title: meta.continuedFrom })}</div>}
              {opening === "loading" ? (
                // Nothing to send into yet: no starting points until it's known what the conversation holds.
                <div className="assistant-opening" role="status">
                  <LoaderCircle size={15} className="spin" aria-hidden /> {t("Loading…")}
                </div>
              ) : opening === "failed" ? (
                <div className="assistant-problem" role="alert">
                  <p>{t("This conversation couldn't be loaded.")}</p>
                  <div className="assistant-problem-actions">
                    <button className="secondary small" onClick={() => void load(threadId, true)}>
                      {t("Try again")}
                    </button>
                    <button className="secondary small" onClick={newConversation}>
                      {t("New conversation")}
                    </button>
                  </div>
                </div>
              ) : entries.length === 0 && !running ? (
                <div className="assistant-welcome">
                  <h3>{t("What would you like to do?")}</h3>
                  <div className="assistant-ideas">
                    {ideas.map((idea) => (
                      <button key={idea} className="assistant-idea" onClick={() => submit(idea)}>
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
            {scroll.away && (
              <button className="assistant-jump" onClick={scroll.jump}>
                <ArrowDown size={13} aria-hidden /> {t("Jump to latest")}
              </button>
            )}
          </div>
          <div className="assistant-compose">
            {problem && (
              <p className="assistant-error" role="alert">
                {problem}
              </p>
            )}
            {notReady ? (
              <div className="assistant-banner" role="status">
                {status.provider === "chatgpt" ? (
                  <>
                    <span>{t("Sign in to use the assistant")}</span>
                    <button className="primary small" disabled={signingIn} aria-busy={signingIn} onClick={() => void signInAgain(false)}>
                      {signingIn && <LoaderCircle size={13} className="spin" aria-hidden />} {t("Continue with ChatGPT")}
                    </button>
                    {signingIn && (
                      <button className="link-button" onClick={() => void assistant.cancelSignIn()}>
                        {t("Cancel")}
                      </button>
                    )}
                  </>
                ) : (
                  <>
                    <span>
                      {t("The assistant is set to use your own {provider} API key. Add it in Settings.", {
                        provider: status.provider === "openai" ? "OpenAI" : "Anthropic",
                      })}
                    </span>
                    <button className="primary small" onClick={onOpenSettings}>
                      {t("Open settings")}
                    </button>
                  </>
                )}
              </div>
            ) : (
              meta?.long &&
              !running && (
                <div className="assistant-banner" role="status">
                  <span>{t("This conversation is getting long, so replies are slower and use more of your plan or key.")}</span>
                  <button className="secondary small" disabled={carrying} onClick={() => void carryOn()}>
                    {t("Continue in a new conversation")}
                  </button>
                </div>
              )
            )}
            {(where.length > 0 || selected.length > 0 || importing) && (
              <div className="assistant-with" role="group" aria-label={t("With this message")}>
                <span className="assistant-with-label">{t("With this message")}</span>
                {where.length > 0 && (
                  <span className="assistant-file">
                    <span className="assistant-with-place">
                      <Place parts={where} />
                    </span>
                    <button
                      className="assistant-file-remove"
                      aria-label={t("Remove {name}", { name: where.join(" · ") })}
                      onClick={() => setWithout((was) => ({ ...was, place: true }))}
                    >
                      <X size={12} />
                    </button>
                  </span>
                )}
                {selected.length > 0 && (
                  <span className="assistant-file selection">
                    <ListChecks size={13} aria-hidden />
                    {tn(selected.length, "{count} selected transaction", "{count} selected transactions")}
                    <button
                      className="assistant-file-remove"
                      aria-label={t("Don't attach the selection")}
                      onClick={() => compose((c) => ({ ...c, selected: [], selectedOf: undefined }))}
                    >
                      <X size={12} />
                    </button>
                  </span>
                )}
                {importing && (
                  <span className="assistant-file">
                    <FileSpreadsheet size={13} aria-hidden />
                    {t("The file being imported")}
                    <button
                      className="assistant-file-remove"
                      aria-label={t("Remove {name}", { name: t("The file being imported") })}
                      onClick={() => setWithout((was) => ({ ...was, import: true }))}
                    >
                      <X size={12} />
                    </button>
                  </span>
                )}
              </div>
            )}
            {capped && selected.length > 0 && <p className="assistant-note">{capped}</p>}
            {(attachments.length > 0 || uploading > 0) && (
              <div className="assistant-files">
                {attachments.map((a) => (
                  <span key={a.id} className="assistant-file" title={a.name}>
                    {fileIcon(a)}
                    {a.name}
                    <button
                      className="assistant-file-remove"
                      aria-label={t("Remove {name}", { name: a.name })}
                      onClick={() => {
                        compose((c) => ({ ...c, attachments: c.attachments.filter((x) => x.id !== a.id) }));
                        // It was only waiting for this message. If forgetting it fails, it goes when the app next starts.
                        void assistant.detach(a.id).catch(() => {});
                      }}
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
                    if (canSend) submit(draft);
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
                  <button className="assistant-send" aria-label={t("Send")} title={t("Send (Enter)")} disabled={!canSend} onClick={() => submit(draft)}>
                    <ArrowUp size={15} />
                  </button>
                )}
              </div>
            </div>
          </div>
          {/* One notice at a time: the plan's comes first, and what gets sent waits behind it. */}
          {planNotice ? (
            <div className="assistant-notice-shade">
              <div className="assistant-notice" role="dialog" aria-modal="true" aria-labelledby="assistant-notice-title">
                <h3 id="assistant-notice-title">{t("You're using your ChatGPT plan")}</h3>
                <p>
                  {t("Messages here count toward your ChatGPT plan's usage, the same as using ChatGPT. You can set a limit for UrBudget in ChatGPT's settings.")}
                </p>
                <div className="assistant-notice-actions">
                  <button className="link-button" onClick={() => void assistant.open("usage")}>
                    {t("Manage usage")}
                  </button>
                  <button className="primary" autoFocus onClick={readPlanNotice}>
                    {t("Got it")}
                  </button>
                </div>
              </div>
            </div>
          ) : (
            held && <SentNotice status={status} onStatus={setStatus} onContinue={proceed} onClose={closeNotice} />
          )}
        </>
      )}
      {preview && <ReportPreview threadId={preview.threadId} proposalId={preview.id} onClose={() => setPreview(null)} />}
      {/* Deleting can't be taken back, so it asks first; drawn outside the panel, as the app's other dialogs are. */}
      {deleting &&
        createPortal(
          <Modal title={t("Delete this conversation?")} close={() => setDeleting(false)}>
            <p>
              {meta?.title
                ? t("“{title}” and everything said in it will be deleted from this computer. This can't be undone.", { title: meta.title })
                : t("Everything said in it will be deleted from this computer. This can't be undone.")}
            </p>
            <div className="modal-actions">
              <button className="secondary" onClick={() => setDeleting(false)}>
                {t("Cancel")}
              </button>
              <button
                className="danger"
                onClick={() => {
                  const id = current.current;
                  setDeleting(false);
                  if (!id) return;
                  newConversation();
                  void assistant.deleteThread(id).catch((e) => toast({ message: errorText(e), tone: "error" }));
                }}
              >
                {t("Delete")}
              </button>
            </div>
          </Modal>,
          document.body,
        )}
      {dragging && (
        <div className="assistant-dropzone">
          <Paperclip size={18} aria-hidden /> {t("Drop to attach")}
        </div>
      )}
    </aside>
  );
}
