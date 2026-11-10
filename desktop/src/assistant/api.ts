import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { read, type BankFixes, type BankSession } from "../bridge";
import type { Patch } from "../patch";
import type { Budget } from "../types";
import { t } from "../i18n";

export type Provider = "chatgpt" | "openai" | "anthropic";
export type Status = {
  provider: Provider;
  /** Signed in, or a key is saved, for the chosen service. */
  ready: boolean;
  account?: { sub: string; email: string; name: string } | null;
  model?: string | null;
  shareMemos: boolean;
  keepHistory: boolean;
  planNoticeSeen: boolean;
  /** The notice of what gets sent, and to whom, hasn't been shown for the chosen service yet. */
  noticeDue: boolean;
  hasOpenaiKey: boolean;
  hasAnthropicKey: boolean;
  /** Where secrets are kept; null when they only last until the app closes. */
  storage: string | null;
  signingIn: boolean;
  mcp: { command: string; args: string[]; config: unknown };
};
export type Attachment = {
  id: string;
  name: string;
  mime: string;
  size: number;
  kind: "image" | "pdf" | "sheet" | "csv" | "text";
};
export type CardRow = {
  label: string;
  detail?: string;
  /** The change for budget moves, the transaction amount otherwise. */
  amount?: number;
  before?: number;
  after?: number;
  badge?: string;
  tone?: "warn";
  /** The part of the change the row shows, to leave out or change before applying. */
  key?: string;
  /** Left out: shown, not applied. */
  skipped?: boolean;
  /** Given another category or amount by the user. */
  edited?: boolean;
  /** What can be changed here: the category it gives (`categoryId`, absent for Ready to Assign), and the amount. */
  edit?: { category: boolean; categoryId?: string; amount: boolean };
};
/** The user's choices on a card, by row key: rows left out, and categories (null: Ready to Assign) or amounts given. */
export type Revision = {
  skip: string[];
  set: { key: string; category?: string | null; amount?: number }[];
};
export type Card = {
  kicker: string;
  rows: CardRow[];
  total?: CardRow;
  stats: { label: string; text?: string; amount?: number }[];
  /** [passed, what was compared] */
  check?: [boolean, string];
  warning?: string;
  note?: string;
  more: number;
  highlight: { month?: string; categories: string[]; transactions: string[] };
};
export type ProposalStatus = "proposed" | "applied" | "discarded" | "undone" | "failed" | "replaced";
/** What a card has to say about itself: it was brought up to date (info), there's something to check (warn), or it can't be applied (error). */
export type CardMessage = { tone: "info" | "warn" | "error"; text: string };
export type Proposal = {
  id: string;
  title: string;
  /**
   * A change (budget, transactions, payees, notes, categories, schedules, delete, fix), a thing
   * made (report, spreadsheet, import, figures) or a question. One the app doesn't know is
   * drawn as a plain card.
   */
  kind: string;
  status: ProposalStatus;
  card: Card;
  /** A thing the assistant made (a report, a spreadsheet, a converted file, figures), not a change. */
  made: boolean;
  /** What the card offers now, in order, the first being the main one. One the app doesn't know is left out. */
  actions: string[];
  message?: CardMessage;
  /** When it was applied (RFC 3339). */
  appliedAt?: string;
  /** The newer proposal made in its place. */
  replacedBy?: string;
  /** Rows in a converted import file. */
  rows: number;
  /** Sheets it can save as a spreadsheet: a spreadsheet's sheets, or a report's tables. */
  sheets: number;
  /** Fixes for a bank file being imported, for the import to apply. */
  fixes?: BankFixes;
  /** What the user left out or changed on the card. */
  choices?: Revision;
};
/**
 * What to offer under an error: signing in with ChatGPT again, the assistant's settings, ChatGPT's
 * usage page, asking again, carrying the conversation on in a new one (this one no longer fits
 * the model), or nothing.
 */
export type ErrorAction = "signIn" | "settings" | "usage" | "retry" | "newConversation" | "none";
/**
 * What went with a message besides its words: the page's key as the app sent it ("" when the
 * user took where they were off the message), the month as YYYY-MM, the account's and the
 * category's names, how many selected transactions were attached, and whether the bank file
 * being imported went too.
 */
export type SentWith = { page: string; month: string; account?: string; category?: string; transactions: number; import: boolean };
export type Entry =
  /** Messages sent before the app kept it have no `with`. */
  | { kind: "user"; text: string; attachments: Attachment[]; with?: SentWith }
  /**
   * `index` names the reply for rating, and `tools` are the tools its turn had called; a reply
   * still arriving has neither until the conversation is read again.
   */
  | { kind: "assistant"; text: string; index?: number; tools?: string[]; rating?: "up" | "down" }
  | { kind: "step"; text: string; failed: boolean }
  | { kind: "proposal"; proposal: Proposal }
  /** Only a conversation's last entry has an `action`: an earlier error's moment has passed. */
  | { kind: "error"; text: string; code: string; action?: ErrorAction }
  /** The reply before it stopped at its length limit. */
  | { kind: "cut" }
  | { kind: "stopped" };
/** Tokens every request of a conversation used, added up. */
export type Usage = { input: number; output: number; cached: number };
export type ThreadView = {
  id: string;
  title: string;
  entries: Entry[];
  running: boolean;
  usage?: Usage;
  /** Its latest request was long enough that carrying on in a new conversation is worth offering. */
  long?: boolean;
  /** The title of the conversation this one carries on. */
  continuedFrom?: string;
  /** What in the budget is waiting that the assistant can help with, offered under the last reply. */
  next?: NextStep[];
};
/** A next step: its button's words, already in the app's language, and the message it sends. */
export type NextStep = { id: string; text: string; prompt: string };
export type Listing = { id: string; title: string; updated: string };
/**
 * What the user is looking at, sent with each message. A `page` of "" says they took where they
 * are off the message, and then there is no month, account or category either.
 */
export type Screen = {
  page: string;
  month?: string;
  accountId?: string | null;
  categoryId?: string | null;
  selected: string[];
  today: string;
  /** The bank file on the Import page, when that's where the user is: the assistant is told its lines. */
  import?: { account: string; content: string; session: BankSession };
};
export type AssistantEvent = { threadId: string } & (
  | { type: "started" }
  | { type: "delta"; text: string }
  /** A piece of the model's reasoning summary, shown while it works. */
  | { type: "thinking"; text: string }
  /** What it is doing now (a lookup under way), already in the app's language. */
  | { type: "activity"; text: string }
  /** A finished reply, with its place in the conversation (`index`, for rating it). */
  | { type: "message"; text: string; index?: number }
  | { type: "step"; text: string; failed: boolean }
  | { type: "proposal"; proposal: Proposal }
  | { type: "error"; text: string; code: string; action?: ErrorAction }
  /** The reply just written stopped at its length limit. */
  | { type: "cut" }
  /** An earlier card as it is now that a newer one corrects it: it takes that card's place. */
  | { type: "replaced"; proposal: Proposal }
  | { type: "done"; stopped?: boolean }
);
export type Model = { id: string; name: string };
/** A report the assistant wrote. `body` is Markdown. */
export type ReportView = { title: string; period?: string | null; body: string; created: string; budget: string };
export type Link = "usage" | "disconnect" | "openaiKeys" | "anthropicKeys";

const watching = new Set<(status: Status) => void>();
/** Passes a status the app is given on to whoever is watching (`onStatus`), whoever asked for it. */
const noted = async (answer: Promise<Status>) => {
  const status = await answer;
  for (const watch of watching) watch(status);
  return status;
};
const marked = new Set<(threadId: string, proposal: Proposal) => void>();

export const assistant = {
  status: () => noted(invoke<Status>("assistant_status")),
  /** Opens the browser and waits for ChatGPT to send the user back. */
  signIn: () => noted(invoke<Status>("assistant_sign_in")),
  cancelSignIn: () => invoke<void>("assistant_cancel_sign_in"),
  signOut: () => noted(invoke<Status>("assistant_sign_out")),
  /** `told` says the user read the notice of what gets sent, for the service in use. */
  settings: (change: Partial<{ provider: Provider; model: string; shareMemos: boolean; keepHistory: boolean; planNoticeSeen: boolean; told: boolean }>) =>
    noted(invoke<Status>("assistant_settings", { change })),
  setKey: (provider: Provider, key: string) => noted(invoke<Status>("assistant_set_key", { provider, key })),
  forgetKey: (provider: Provider) => noted(invoke<Status>("assistant_forget_key", { provider })),
  /**
   * Hears every status the app is given, so a change made in Settings (another service, say)
   * reaches the open panel at once. Returns how to stop listening.
   */
  onStatus: (handler: (status: Status) => void) => {
    watching.add(handler);
    return () => {
      watching.delete(handler);
    };
  },
  models: (refresh = false) => invoke<Model[]>("assistant_models", { refresh }),
  threads: () => invoke<Listing[]>("assistant_threads"),
  /** The conversation, with its waiting cards brought up to date with `budget` as the window has it. */
  thread: (budget: Budget, id: string) => read<ThreadView>(budget, "assistant_thread", { id }),
  deleteThread: (id: string) => invoke<void>("assistant_delete_thread", { id }),
  /** Gives a conversation another title, of 1 to 80 characters. */
  renameThread: (id: string, title: string) => invoke<void>("assistant_rename_thread", { id, title }),
  /** Starts a conversation that begins with what was said last in this one, and returns its id. */
  continue: (threadId: string) => invoke<string>("assistant_continue", { threadId }),
  /** The user's thumb on a reply (by its `index`): up, down, or null to take it off. It stays on this computer. */
  rate: (threadId: string, index: number, up: boolean | null) => invoke<void>("assistant_rate", { threadId, index, up }),
  deleteAll: () => invoke<void>("assistant_delete_all"),
  /** Keeps a file for the next message, until it's sent or taken off again. `data` is base64. */
  attach: (name: string, data: string) => invoke<Attachment>("assistant_attach", { name, data }),
  /** Forgets a file taken off the message before it was sent. */
  detach: (id: string) => invoke<void>("assistant_detach", { id }),
  /** Sends a message; the reply arrives as events. Returns the conversation id. */
  send: (budget: Budget, threadId: string | null, text: string, attachments: string[], screen: Screen) =>
    read<string>(budget, "assistant_send", { threadId, text, attachments, screen }),
  /** Asks again after an error, or for the rest of a reply that was cut short, without a new message. */
  retry: (budget: Budget, threadId: string, screen: Screen) => read<void>(budget, "assistant_retry", { threadId, screen }),
  stop: (threadId: string) => invoke<void>("assistant_stop", { threadId }),
  /**
   * The proposal's change as a patch for `budget`, or a refreshed card when the budget moved.
   * The card isn't marked applied here: that's `mark`, once the change is in the budget.
   */
  apply: (budget: Budget, threadId: string, proposalId: string) =>
    read<{ patch: Patch | null; proposal: Proposal }>(budget, "assistant_apply", { threadId, proposalId, settle: false }),
  /** The proposal as the user chose on its card, made again on `budget`: every row, those left out marked. */
  revise: (budget: Budget, threadId: string, proposalId: string, revision: Revision) =>
    read<Proposal>(budget, "assistant_revise", { threadId, proposalId, revision }),
  /**
   * Records what was done with a card (applied, set aside, put back, undone) and returns it as
   * it is now; the core refuses a step that can't follow from where the card stands. `note` is
   * what the app has to say about it, which becomes the card's message.
   */
  mark: async (threadId: string, proposalId: string, status: ProposalStatus, note?: string) => {
    const proposal = await invoke<Proposal>("assistant_mark", { threadId, proposalId, status, note });
    for (const hear of marked) hear(threadId, proposal);
    return proposal;
  },
  /**
   * Hears of every card the app marks, whoever marked it, so one that changes outside the panel
   * (the window's Undo took its change back, its import was finished) is up to date in the open
   * panel. Returns how to stop listening.
   */
  onMarked: (handler: (threadId: string, proposal: Proposal) => void) => {
    marked.add(handler);
    return () => {
      marked.delete(handler);
    };
  },
  /** The change that makes the budget's notes for the assistant exactly `notes`, as a patch for `budget`. */
  notes: (budget: Budget, notes: string[]) => read<Patch>(budget, "assistant_notes", { notes }),
  importFile: (threadId: string, proposalId: string) =>
    invoke<{ account: string; name: string; csv: string }>("assistant_import", { threadId, proposalId }),
  /** Saves the converted file where the user chooses; null when they cancel. */
  saveImport: (threadId: string, proposalId: string) => invoke<string | null>("assistant_save_import", { threadId, proposalId }),
  open: (link: Link) => invoke<void>("assistant_open", { link }),
  /** Sends only the text the user approved in the report form; returns its support reference. */
  reportReply: (text: string, reason: string) => invoke<string>("assistant_report_reply", { text, reason }),
  report: (threadId: string, proposalId: string) => invoke<ReportView>("assistant_report", { threadId, proposalId }),
  /**
   * Saves the card's spreadsheet (or a report's tables) where the user chooses, with data sheets
   * filled from `budget`, and returns its path, or null when they cancel. `open` opens it in the
   * user's spreadsheet app instead, reusing the file saved last.
   */
  spreadsheet: (budget: Budget, threadId: string, proposalId: string, open: boolean) =>
    read<string | null>(budget, "assistant_spreadsheet", { threadId, proposalId, open }),
  /** Opens the system print dialog for the window. */
  print: () => invoke<void>("assistant_print"),
  onEvent: (handler: (e: AssistantEvent) => void): Promise<UnlistenFn> =>
    listen<AssistantEvent>("assistant://event", (e) => handler(e.payload)),
};

const dataUrl = (file: Blob, name: string) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error(t("{name} couldn't be read.", { name })));
    reader.readAsDataURL(file);
  });
/** Photos over this size are scaled down first; some models refuse images above 5 MB. */
const LARGE_IMAGE = 3_500_000;
const MAX_SIDE = 2400;
/** A large photo as a JPEG of at most MAX_SIDE pixels, or null when it can't be scaled here. */
async function smaller(file: File, url: string): Promise<string | null> {
  const image = new Image();
  const loaded = new Promise<void>((resolve, reject) => {
    image.onload = () => resolve();
    image.onerror = () => reject(new Error("unreadable"));
  });
  image.src = url;
  try {
    await loaded;
  } catch {
    return null;
  }
  const scale = Math.min(1, MAX_SIDE / Math.max(image.naturalWidth, image.naturalHeight));
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(image.naturalWidth * scale);
  canvas.height = Math.round(image.naturalHeight * scale);
  const context = canvas.getContext("2d");
  if (!context || !canvas.width) return null;
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.85));
  return blob ? dataUrl(blob, file.name) : null;
}
/**
 * A file ready to attach: its name (a scaled photo becomes .jpg) and contents as base64,
 * without the data URL prefix.
 */
export async function fileData(file: File): Promise<{ name: string; data: string }> {
  let url = await dataUrl(file, file.name);
  let name = file.name;
  if (file.type.startsWith("image/") && file.size > LARGE_IMAGE) {
    const scaled = await smaller(file, url);
    if (scaled) {
      url = scaled;
      name = name.replace(/\.[^.]+$/, "") + ".jpg";
    }
  }
  return { name, data: url.replace(/^data:[^,]*,/, "") };
}

export const ACCEPT = ".png,.jpg,.jpeg,.gif,.webp,.pdf,.xlsx,.xlsm,.xls,.xlsb,.ods,.csv,.tsv,.txt,.ofx,.qfx,.qif";
