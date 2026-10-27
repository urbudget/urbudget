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
export type ProposalStatus = "proposed" | "applied" | "discarded" | "undone" | "failed";
export type Proposal = {
  id: string;
  title: string;
  kind: "budget" | "transactions" | "payees" | "import" | "report" | "spreadsheet" | "fix";
  status: ProposalStatus;
  card: Card;
  problem?: string;
  /** Rows in a converted import file. */
  rows: number;
  /** Sheets it can save as a spreadsheet: a spreadsheet's sheets, or a report's tables. */
  sheets: number;
  /** Fixes for a bank file being imported, for the import to apply. */
  fixes?: BankFixes;
  /** What the user left out or changed on the card. */
  choices?: Revision;
};
export type Entry =
  | { kind: "user"; text: string; attachments: Attachment[]; selected?: number }
  | { kind: "assistant"; text: string }
  | { kind: "step"; text: string; failed: boolean }
  | { kind: "proposal"; proposal: Proposal }
  | { kind: "error"; text: string; code: string }
  | { kind: "stopped" };
export type ThreadView = { id: string; title: string; entries: Entry[]; running: boolean };
export type Listing = { id: string; title: string; updated: string };
/** What the user is looking at, sent with each message. */
export type Screen = {
  page: string;
  month: string;
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
  | { type: "message"; text: string }
  | { type: "step"; text: string; failed: boolean }
  | { type: "proposal"; proposal: Proposal }
  | { type: "error"; text: string; code: string }
  | { type: "done"; stopped?: boolean }
);
export type Model = { id: string; name: string };
/** A report the assistant wrote. `body` is Markdown. */
export type ReportView = { title: string; period?: string | null; body: string; created: string; budget: string };
export type Link = "usage" | "disconnect" | "openaiKeys" | "anthropicKeys";

export const assistant = {
  status: () => invoke<Status>("assistant_status"),
  /** Opens the browser and waits for ChatGPT to send the user back. */
  signIn: () => invoke<Status>("assistant_sign_in"),
  cancelSignIn: () => invoke<void>("assistant_cancel_sign_in"),
  signOut: () => invoke<Status>("assistant_sign_out"),
  settings: (change: Partial<{ provider: Provider; model: string; shareMemos: boolean; keepHistory: boolean; planNoticeSeen: boolean }>) =>
    invoke<Status>("assistant_settings", { change }),
  setKey: (provider: Provider, key: string) => invoke<Status>("assistant_set_key", { provider, key }),
  forgetKey: (provider: Provider) => invoke<Status>("assistant_forget_key", { provider }),
  models: (refresh = false) => invoke<Model[]>("assistant_models", { refresh }),
  threads: () => invoke<Listing[]>("assistant_threads"),
  thread: (id: string) => invoke<ThreadView>("assistant_thread", { id }),
  deleteThread: (id: string) => invoke<void>("assistant_delete_thread", { id }),
  deleteAll: () => invoke<void>("assistant_delete_all"),
  /** Saves a file for the next message. `data` is base64. */
  attach: (name: string, data: string) => invoke<Attachment>("assistant_attach", { name, data }),
  /** Sends a message; the reply arrives as events. Returns the conversation id. */
  send: (budget: Budget, threadId: string | null, text: string, attachments: string[], screen: Screen) =>
    read<string>(budget, "assistant_send", { threadId, text, attachments, screen }),
  /** Asks again after an error, without a new message. */
  retry: (budget: Budget, threadId: string, screen: Screen) => read<void>(budget, "assistant_retry", { threadId, screen }),
  stop: (threadId: string) => invoke<void>("assistant_stop", { threadId }),
  /** The proposal's change as a patch for `budget`, or a refreshed card when the budget moved. */
  apply: (budget: Budget, threadId: string, proposalId: string) =>
    read<{ patch: Patch | null; proposal: Proposal }>(budget, "assistant_apply", { threadId, proposalId }),
  /** The proposal as the user chose on its card, made again on `budget`: every row, those left out marked. */
  revise: (budget: Budget, threadId: string, proposalId: string, revision: Revision) =>
    read<Proposal>(budget, "assistant_revise", { threadId, proposalId, revision }),
  mark: (threadId: string, proposalId: string, status: ProposalStatus) =>
    invoke<Proposal>("assistant_mark", { threadId, proposalId, status }),
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
