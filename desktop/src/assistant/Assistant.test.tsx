import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { AssistantEvent, Proposal, Status } from "./api";

let emit: (e: AssistantEvent) => void = () => {};
/** A status the app was given somewhere else (Settings), as the panel hears of it. */
let tell: (s: Status) => void = () => {};
/** A card the window marked outside the panel (its change undone, its import finished), as the panel hears of it. */
let marked: (threadId: string, p: Proposal) => void = () => {};
vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    assistant: {
      status: vi.fn(),
      onStatus: vi.fn((handler: (s: Status) => void) => {
        tell = handler;
        return () => {};
      }),
      signIn: vi.fn(),
      cancelSignIn: vi.fn(),
      settings: vi.fn(),
      thread: vi.fn(),
      threads: vi.fn(async () => []),
      send: vi.fn(),
      retry: vi.fn(),
      stop: vi.fn(),
      mark: vi.fn(),
      onMarked: vi.fn((handler: (threadId: string, p: Proposal) => void) => {
        marked = handler;
        return () => {};
      }),
      revise: vi.fn(),
      notes: vi.fn(),
      models: vi.fn(async () => []),
      attach: vi.fn(),
      detach: vi.fn(async () => {}),
      open: vi.fn(async () => {}),
      saveImport: vi.fn(),
      deleteThread: vi.fn(async () => {}),
      renameThread: vi.fn(async () => {}),
      continue: vi.fn(),
      rate: vi.fn(async () => {}),
      reportReply: vi.fn(),
      report: vi.fn(),
      print: vi.fn(async () => {}),
      spreadsheet: vi.fn(),
      onEvent: vi.fn(async (handler: (e: AssistantEvent) => void) => {
        emit = handler;
        return () => {};
      }),
    },
  };
});
vi.mock("../bridge", () => ({ api: { reveal: vi.fn() } }));
import { assistant } from "./api";
import { AssistantBoundary, AssistantPanel, Markdown, place, REFRESH, suggestions, thinkingTitle, widest, type Request } from "./Assistant";
import { AssistantSettings } from "./AssistantSettings";
import { forgetComposers } from "./composer";
import { following } from "./follow";
import { printReport } from "./Report";
import { demoBudget } from "../demo";
import { ToastProvider } from "../ui";
import type { Budget, Summary } from "../types";

const ready: Status = {
  provider: "chatgpt",
  ready: true,
  account: { sub: "u1", email: "pat@example.com", name: "Pat" },
  model: null,
  shareMemos: true,
  keepHistory: true,
  planNoticeSeen: true,
  noticeDue: false,
  hasOpenaiKey: false,
  hasAnthropicKey: false,
  storage: "Credential Manager",
  signingIn: false,
  mcp: { command: "UrBudget.exe", args: ["--mcp"], config: {} },
};
const summary = {
  readyToAssign: 115000,
  assigned: 0,
  activity: 0,
  available: 0,
  income: 0,
  netWorth: 0,
  accounts: [],
  categories: [
    { id: "dining", assigned: 120000, activity: -144000, available: -24000, carried: 0 },
    { id: "rent", assigned: 500000, activity: -500000, available: 0, carried: 0 },
  ],
} as Summary;
const proposal: Proposal = {
  id: "p_1",
  title: "Cover dining",
  kind: "budget",
  status: "proposed",
  made: false,
  actions: ["apply", "askChanges", "discard", "show"],
  rows: 0,
  sheets: 0,
  card: {
    kicker: "Proposal · Budget",
    rows: [{ label: "Dining out", amount: 24000, before: -24000, after: 0 }],
    total: { label: "Ready to assign", amount: -24000, before: 115000, after: 91000 },
    stats: [],
    more: 0,
    highlight: { month: "2026-10", categories: ["dining"], transactions: [] },
  },
};

/** The card as the core has it once its change is applied. */
const applied = (p: Proposal): Proposal => ({ ...p, status: "applied", actions: ["undo", "show"] });
function panel(props: Partial<Parameters<typeof AssistantPanel>[0]> = {}) {
  const handlers = {
    onApply: vi.fn(async (_t: string, p: Proposal) => applied(p)),
    onUndo: vi.fn(async (_t: string, p: Proposal): Promise<Proposal> => ({ ...p, status: "undone", actions: ["apply", "askChanges", "discard"] })),
    onHighlight: vi.fn(),
    onOpenSettings: vi.fn(),
    onShow: vi.fn(),
  };
  render(
    <ToastProvider>
      <AssistantPanel
        budget={demoBudget()}
        budgetId="b1"
        summary={summary}
        screen={{ page: "budget", month: "2026-10" }}
        request={null}
        width={380}
        onResize={() => {}}
        onClose={() => {}}
        canUndo={() => true}
        onReviewImport={() => {}}
        {...handlers}
        {...props}
      />
    </ToastProvider>,
  );
  return handlers;
}

const household = demoBudget();
/**
 * The panel as the window hosts it: there only while open, and given a request that the window
 * forgets once the panel has taken it.
 */
function Hosted({ open = true, ask = null, budgetId = "b1" }: { open?: boolean; ask?: Request | null; budgetId?: string }) {
  const [taken, setTaken] = useState<number | null>(null);
  return (
    <ToastProvider>
      {open && (
        <AssistantPanel
          budget={household}
          budgetId={budgetId}
          summary={summary}
          screen={{ page: "budget", month: "2026-10" }}
          request={ask && ask.key !== taken ? ask : null}
          onRequestTaken={() => setTaken(ask?.key ?? null)}
          width={380}
          onResize={() => {}}
          onClose={() => {}}
          onOpenSettings={() => {}}
          onApply={async (_t, p) => p}
          onUndo={async (_t, p) => p}
          canUndo={() => true}
          onReviewImport={() => {}}
          onHighlight={() => {}}
        />
      )}
    </ToastProvider>
  );
}
const messageBox = async () => (await screen.findByLabelText("Message the assistant")) as HTMLTextAreaElement;

afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  forgetComposers();
});

it("asks to sign in with ChatGPT before anything else", async () => {
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, ready: false, account: null });
  vi.mocked(assistant.signIn).mockResolvedValue(ready);
  panel();
  fireEvent.click(await screen.findByRole("button", { name: "Continue with ChatGPT" }));
  expect(assistant.signIn).toHaveBeenCalled();
  expect(await screen.findByLabelText("Message the assistant")).toBeTruthy();
  expect(screen.getByText("Using ChatGPT plan")).toBeTruthy();
});

it("says while a message is being written or sent, which an automatic update waits for", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Cover", entries: [], running: true });
  const onWorking = vi.fn();
  panel({ onWorking });
  const box = await screen.findByLabelText("Message the assistant");
  expect(onWorking).toHaveBeenLastCalledWith(false);
  fireEvent.change(box, { target: { value: "Cover what I overspent" } });
  expect(onWorking).toHaveBeenLastCalledWith(true);
  // Sent, and waiting for the reply: still at work.
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.send).toHaveBeenCalled());
  expect(onWorking).toHaveBeenLastCalledWith(true);
  cleanup();
  expect(onWorking).toHaveBeenLastCalledWith(false);
});

it("offers starting points from the budget and sends one with the screen", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Cover", entries: [], running: true });
  panel();
  fireEvent.click(await screen.findByRole("button", { name: "Cover the overspent category" }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalled());
  const [, thread, text, files, screenArg] = vi.mocked(assistant.send).mock.calls[0];
  expect(thread).toBeNull();
  expect(text).toBe("Cover the overspent category");
  expect(files).toEqual([]);
  expect(screenArg).toMatchObject({ page: "budget", month: "2026-10", selected: [] });
});

it("streams a reply, shows steps and a proposal, and applies it", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_1",
    title: "Cover",
    entries: [{ kind: "user", text: "Cover what I overspent", attachments: [] }],
    running: true,
  });
  const handlers = panel();
  const box = await screen.findByLabelText("Message the assistant");
  fireEvent.change(box, { target: { value: "Cover what I overspent" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith(expect.anything(), "t_1"));
  act(() => {
    emit({ threadId: "t_1", type: "started" });
    emit({ threadId: "t_1", type: "thinking", text: "**Checking October**\n\nLooking at the categories" });
  });
  // While the model thinks, its latest heading shows instead of a blank wait.
  expect(await screen.findByText("Checking October")).toBeTruthy();
  act(() => {
    emit({ threadId: "t_1", type: "step", text: "Checked October 2026: 1 category is overspent", failed: false });
    emit({ threadId: "t_1", type: "delta", text: "Dining out is " });
    emit({ threadId: "t_1", type: "delta", text: "**240.00** over." });
  });
  // The text is revealed over a few frames, never with stray markers.
  expect((await screen.findByText("240.00")).tagName).toBe("STRONG");
  expect(screen.queryByText(/\*\*/)).toBeNull();
  const reply = screen.getByText("240.00").closest(".assistant-reply");
  act(() => {
    emit({ threadId: "t_1", type: "message", text: "Dining out is **240.00** over." });
    emit({ threadId: "t_1", type: "proposal", proposal });
    // Another conversation's events are ignored.
    emit({ threadId: "t_other", type: "delta", text: "elsewhere" });
  });
  expect(screen.getByText("Checked October 2026: 1 category is overspent")).toBeTruthy();
  expect(screen.queryByText("elsewhere")).toBeNull();
  // The finished reply is the same element that streamed, shown once.
  expect(screen.getByText("240.00").closest(".assistant-reply")).toBe(reply);
  expect(document.querySelectorAll(".assistant-reply")).toHaveLength(1);
  expect(screen.getByText("Cover dining")).toBeTruthy();
  await waitFor(() => expect(handlers.onHighlight).toHaveBeenLastCalledWith(proposal.card.highlight));
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  await waitFor(() => expect(handlers.onApply).toHaveBeenCalledWith("t_1", proposal));
  // Applied, it folds to a line with what can still be done with it.
  expect(await screen.findByText("Applied")).toBeTruthy();
  expect(screen.getByText("Cover dining")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  expect(screen.getByRole("button", { name: "Undo" })).toBeTruthy();
  // Nothing waits any more, so nothing is marked on the page.
  await waitFor(() => expect(handlers.onHighlight).toHaveBeenLastCalledWith(null));
  // Its Undo is the window's: the card is what that leaves it.
  fireEvent.click(screen.getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(handlers.onUndo).toHaveBeenCalledWith("t_1", applied(proposal)));
  expect(await screen.findByText("Undone")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Apply again" })).toBeTruthy();
});

it("explains a usage limit and links to the plan's usage page", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      { kind: "error", text: "You've reached your ChatGPT plan's usage limit.", code: "limit", action: "usage" },
    ],
  });
  panel();
  expect(await screen.findByText("Usage limit reached")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Manage usage" }));
  expect(assistant.open).toHaveBeenCalledWith("usage");
});

it("asks again after an error that says so, and offers nothing under an earlier one", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.retry).mockResolvedValue(undefined);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      // Only the error a conversation ends on says what helps.
      { kind: "error", text: "The service was busy.", code: "busy" },
      { kind: "user", text: "Hi again", attachments: [] },
      { kind: "error", text: "The connection dropped.", code: "network", action: "retry" },
    ],
  });
  panel();
  expect(await screen.findByText("The connection dropped.")).toBeTruthy();
  expect(within(screen.getByText("The service was busy.").closest(".assistant-problem") as HTMLElement).queryByRole("button")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(assistant.retry).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.retry).mock.calls[0][1]).toBe("t_9");
});

it("offers nothing under an error that asking again wouldn't help", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      { kind: "error", text: "The assistant can't help with that.", code: "refused", action: "none" },
    ],
  });
  panel();
  const problem = (await screen.findByText("Declined")).closest(".assistant-problem") as HTMLElement;
  expect(within(problem).queryByRole("button")).toBeNull();
});

it("heads an error by its code and leaves what to offer to the core", async () => {
  // An API key's limit isn't the plan's: there is no usage page to open, only asking again.
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, provider: "openai" });
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      { kind: "error", text: "Your OpenAI key has reached its limit.", code: "limit", action: "retry" },
    ],
  });
  panel();
  expect(await screen.findByText("Usage limit reached")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Manage usage" })).toBeNull();
});

it("opens the assistant's settings from an error that needs them", async () => {
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, provider: "anthropic" });
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      { kind: "error", text: "That model isn't available to your key.", code: "model", action: "settings" },
    ],
  });
  const handlers = panel();
  fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
  expect(handlers.onOpenSettings).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
});

it("signs in again from an error, then asks again by itself", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.retry).mockResolvedValue(undefined);
  let signedIn: (s: Status) => void = () => {};
  vi.mocked(assistant.signIn).mockReturnValue(new Promise<Status>((resolve) => (signedIn = resolve)));
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_9",
    title: "Hi",
    running: false,
    entries: [
      { kind: "user", text: "Hi", attachments: [] },
      { kind: "error", text: "Sign in with ChatGPT to use the assistant.", code: "signIn", action: "signIn" },
    ],
  });
  panel();
  const again = await screen.findByRole("button", { name: "Continue with ChatGPT" });
  fireEvent.click(again);
  // Busy while the browser has the sign-in, which can be given up.
  expect(again).toHaveProperty("disabled", true);
  expect(again.getAttribute("aria-busy")).toBe("true");
  expect(assistant.retry).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(assistant.cancelSignIn).toHaveBeenCalled();
  await act(async () => signedIn(ready));
  await waitFor(() => expect(assistant.retry).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.retry).mock.calls[0][1]).toBe("t_9");
  expect(assistant.signIn).toHaveBeenCalledTimes(1);
});

it("offers a live error's way out once the reply has ended", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  const asked = { kind: "user" as const, text: "Hi", attachments: [] };
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Hi", entries: [asked], running: true });
  panel();
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "Hi" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith(expect.anything(), "t_1"));
  act(() => {
    emit({ threadId: "t_1", type: "started" });
    emit({ threadId: "t_1", type: "error", text: "The connection dropped.", code: "network", action: "retry" });
  });
  expect(await screen.findByText("The connection dropped.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_1",
    title: "Hi",
    entries: [asked, { kind: "error", text: "The connection dropped.", code: "network", action: "retry" }],
    running: false,
  });
  act(() => emit({ threadId: "t_1", type: "done" }));
  expect(await screen.findByRole("button", { name: "Try again" })).toBeTruthy();
  expect(screen.getAllByText("The connection dropped.")).toHaveLength(1);
});

it("keeps what was written of a reply the user stopped, shown once", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.stop).mockResolvedValue(undefined);
  const asked = { kind: "user" as const, text: "How is dining going?", attachments: [] };
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Dining", entries: [asked], running: true });
  panel();
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "How is dining going?" } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith(expect.anything(), "t_1"));
  act(() => {
    emit({ threadId: "t_1", type: "started" });
    emit({ threadId: "t_1", type: "delta", text: "You've spent 1,240.00 of" });
  });
  const reply = (await screen.findByText("You've spent 1,240.00 of")).closest(".assistant-reply");
  // Stopping keeps the text so far in the conversation, followed by the mark.
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_1",
    title: "Dining",
    entries: [asked, { kind: "assistant", text: "You've spent 1,240.00 of" }, { kind: "stopped" }],
    running: false,
  });
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(assistant.stop).toHaveBeenCalledWith("t_1");
  act(() => emit({ threadId: "t_1", type: "done", stopped: true }));
  expect(await screen.findByText("Stopped")).toBeTruthy();
  // The text that streamed is the same element, and there is one of it.
  expect(document.querySelectorAll(".assistant-reply")).toHaveLength(1);
  expect(screen.getByText("You've spent 1,240.00 of").closest(".assistant-reply")).toBe(reply);
  expect(reply?.classList.contains("typing")).toBe(false);
  expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
});

it("tells a first-time user the plan is being used", async () => {
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, planNoticeSeen: false });
  vi.mocked(assistant.settings).mockResolvedValue(ready);
  panel();
  expect(await screen.findByText("You're using your ChatGPT plan")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Got it" }));
  expect(assistant.settings).toHaveBeenCalledWith({ planNoticeSeen: true });
  await waitFor(() => expect(screen.queryByText("You're using your ChatGPT plan")).toBeNull());
});

it("attaches the selection an entry point sends", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  panel({ request: { key: 1, selected: ["t1", "t2"] } });
  expect(await screen.findByText("2 selected transactions")).toBeTruthy();
  // It's one of the things the line over the field says go with the message.
  expect(within(screen.getByRole("group", { name: "With this message" })).getByText("2 selected transactions")).toBeTruthy();
});

it("takes an entry point's request once, not again each time the panel reopens", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.attach).mockResolvedValue({ id: "file_1", name: "statement.csv", mime: "text/csv", size: 11, kind: "csv" });
  // Forgetting a file that's already gone is nothing to tell the user about.
  vi.mocked(assistant.detach).mockRejectedValue(new Error("No attachment has the id file_1."));
  const ask: Request = {
    key: 1,
    text: "Help me import this statement.",
    selected: ["t1", "t2"],
    files: [new File(["date,amount"], "statement.csv", { type: "text/csv" })],
  };
  const view = render(<Hosted ask={ask} />);
  const box = await messageBox();
  await waitFor(() => expect(box.value).toBe("Help me import this statement."));
  expect(screen.getByText("2 selected transactions")).toBeTruthy();
  // The user takes all of it off again: the file is forgotten, not left waiting.
  fireEvent.click(await screen.findByRole("button", { name: "Remove statement.csv" }));
  expect(assistant.detach).toHaveBeenCalledWith("file_1");
  fireEvent.click(screen.getByRole("button", { name: "Don't attach the selection" }));
  fireEvent.change(box, { target: { value: "" } });
  view.rerender(<Hosted ask={ask} open={false} />);
  expect(screen.queryByLabelText("Message the assistant")).toBeNull();
  view.rerender(<Hosted ask={ask} />);
  const reopened = await messageBox();
  expect(reopened).not.toBe(box);
  // Nothing comes back: no text, no selection, no file on its way.
  expect(reopened.value).toBe("");
  expect(screen.queryByText("2 selected transactions")).toBeNull();
  expect(screen.queryByText("Attaching…")).toBeNull();
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
  expect(screen.queryByText("statement.csv")).toBeNull();
  expect(assistant.attach).toHaveBeenCalledTimes(1);
  expect(screen.queryByText(/No attachment/)).toBeNull();
});

it("keeps a message being written while the panel is closed, one for each budget", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.attach).mockResolvedValue({ id: "file_2", name: "receipt.png", mime: "image/png", size: 4, kind: "image" });
  const view = render(<Hosted />);
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "What did this receipt go to" } });
  fireEvent.change(document.querySelector('input[type="file"]')!, { target: { files: [new File(["\x89PNG"], "receipt.png", { type: "image/png" })] } });
  expect(await screen.findByText("receipt.png")).toBeTruthy();
  view.rerender(<Hosted open={false} />);
  view.rerender(<Hosted />);
  const reopened = await messageBox();
  expect(reopened).not.toBe(box);
  expect(reopened.value).toBe("What did this receipt go to");
  expect(screen.getByText("receipt.png")).toBeTruthy();
  expect(assistant.attach).toHaveBeenCalledTimes(1);
  // Another budget's box starts empty, and this one's is there on the way back.
  view.rerender(<Hosted budgetId="b2" />);
  expect((await messageBox()).value).toBe("");
  expect(screen.queryByText("receipt.png")).toBeNull();
  view.rerender(<Hosted />);
  expect((await messageBox()).value).toBe("What did this receipt go to");
  expect(screen.getByText("receipt.png")).toBeTruthy();
});

it("adds an entry point's request after a message already being written", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  const view = render(<Hosted />);
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "Also, is rent paid?\n" } });
  view.rerender(<Hosted ask={{ key: 2, text: "Categorize the 2 new transactions in Checking." }} />);
  await waitFor(() => expect(box.value).toBe("Also, is rent paid?\nCategorize the 2 new transactions in Checking."));
  expect(assistant.send).not.toHaveBeenCalled();
});

it("says what gets sent before a first message, and sends nothing until the user goes on", async () => {
  let now: Status = { ...ready, noticeDue: true };
  vi.mocked(assistant.status).mockImplementation(async () => now);
  vi.mocked(assistant.settings).mockImplementation(async (change) => {
    now = { ...now, shareMemos: change.shareMemos ?? now.shareMemos, noticeDue: change.told ? false : now.noticeDue };
    return now;
  });
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Cover", entries: [], running: true });
  panel();
  // A starting point asks first, as any message does.
  fireEvent.click(await screen.findByRole("button", { name: "Cover the overspent category" }));
  let notice = screen.getByRole("dialog", { name: "What gets sent" });
  expect(notice.textContent).toContain("go to OpenAI.");
  expect(notice.textContent).toContain("are kept on this computer");
  const go = within(notice).getByRole("button", { name: "Continue" });
  expect(document.activeElement).toBe(go);
  // The keyboard stays in the notice, and Esc is "Not now".
  const memos = within(notice).getByRole("switch", { name: /Share memos/ });
  fireEvent.keyDown(go, { key: "Tab" });
  expect(document.activeElement).toBe(memos);
  fireEvent.keyDown(memos, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(go);
  // Enter still held down from sending doesn't press Continue.
  expect(fireEvent.keyDown(go, { key: "Enter", repeat: true })).toBe(false);
  fireEvent.keyDown(go, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();

  const box = await messageBox();
  fireEvent.change(box, { target: { value: "Cover what I overspent" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  notice = screen.getByRole("dialog", { name: "What gets sent" });
  fireEvent.click(within(notice).getByRole("button", { name: "Not now" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(box.value).toBe("Cover what I overspent");
  expect(assistant.settings).not.toHaveBeenCalled();
  expect(assistant.send).not.toHaveBeenCalled();

  fireEvent.keyDown(box, { key: "Enter" });
  notice = screen.getByRole("dialog", { name: "What gets sent" });
  expect(assistant.send).not.toHaveBeenCalled();
  // Memos can be kept back right there.
  fireEvent.click(within(notice).getByRole("switch", { name: /Share memos/ }));
  expect(assistant.settings).toHaveBeenCalledWith({ shareMemos: false });
  await waitFor(() => expect(within(notice).getByRole("switch", { name: /Share memos/ })).toHaveProperty("checked", false));
  expect(assistant.send).not.toHaveBeenCalled();
  fireEvent.click(within(notice).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledTimes(1));
  expect(assistant.settings).toHaveBeenLastCalledWith({ told: true });
  expect(vi.mocked(assistant.send).mock.calls[0][2]).toBe("Cover what I overspent");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(box.value).toBe("");
});

it("asks again for a service chosen in Settings while the panel is open", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  panel();
  const box = await messageBox();
  expect(screen.getByText("Using ChatGPT plan")).toBeTruthy();
  // Settings is given the status after its change, and the panel hears of it there and then.
  act(() => tell({ ...ready, provider: "anthropic", account: null, noticeDue: true }));
  expect(screen.getByText("Using your Anthropic key")).toBeTruthy();
  fireEvent.change(box, { target: { value: "Hi" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(screen.getByRole("dialog", { name: "What gets sent" }).textContent).toContain("go to Anthropic.");
  expect(assistant.send).not.toHaveBeenCalled();
});

it("shows the plan's notice first, and what gets sent once that is read", async () => {
  let now: Status = { ...ready, planNoticeSeen: false, noticeDue: true };
  vi.mocked(assistant.status).mockImplementation(async () => now);
  vi.mocked(assistant.settings).mockImplementation(async (change) => {
    now = { ...now, planNoticeSeen: change.planNoticeSeen ?? now.planNoticeSeen };
    return now;
  });
  panel();
  // The keyboard can still reach the message box under the plan's notice.
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "Hi" } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(screen.getByRole("dialog", { name: "You're using your ChatGPT plan" }).textContent).toContain("a limit for UrBudget in");
  fireEvent.click(screen.getByRole("button", { name: "Got it" }));
  expect((await screen.findByRole("dialog", { name: "What gets sent" })).textContent).toContain("go to OpenAI.");
  expect(screen.getAllByRole("dialog")).toHaveLength(1);
  expect(assistant.send).not.toHaveBeenCalled();
});

it("groups the lookups behind a reply", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_2");
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_2",
    title: "Groceries",
    running: false,
    entries: [
      { kind: "user", text: "Groceries since June?", attachments: [] },
      { kind: "step", text: "Looked at your accounts and categories", failed: false },
      { kind: "step", text: "Searched transactions: 40 found", failed: false },
      { kind: "step", text: "Ran a spending report: June to October 2026", failed: false },
      { kind: "assistant", text: "You spent **4,070.00** on groceries." },
    ],
  });
  panel();
  expect(await screen.findByText("Ran a spending report: June to October 2026")).toBeTruthy();
  expect(screen.queryByText("Searched transactions: 40 found")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: /2 more/ }));
  expect(screen.getByText("Searched transactions: 40 found")).toBeTruthy();
  // Opening a conversation shows it as it was, without replaying the text.
  expect(screen.getByText("4,070.00").tagName).toBe("STRONG");
  expect(screen.getByRole("button", { name: "Report a problem" })).toHaveProperty("disabled", false);
});

it("previews a report and prints only the report", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_3");
  const report: Proposal = {
    id: "p_r",
    title: "October 2026 in review",
    kind: "report",
    status: "proposed",
    made: true,
    actions: ["preview", "print", "excel", "remove"],
    rows: 0,
    sheets: 1,
    card: { kicker: "Report", rows: [{ label: "Spending" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  };
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_3",
    title: "Report",
    running: false,
    entries: [
      { kind: "user", text: "Write me a report for October", attachments: [] },
      { kind: "proposal", proposal: report },
    ],
  });
  vi.mocked(assistant.report).mockResolvedValue({
    title: "October 2026 in review",
    period: "October 2026",
    body: ["October went well.", "", "## Spending", "", "| Category | Spent |", "|---|---:|", "| Dining out | 1,240.00 |"].join("\n"),
    created: "2026-10-16T10:00:00Z",
    budget: "Household",
  });
  panel();
  fireEvent.click(await screen.findByRole("button", { name: "Preview" }));
  const dialog = await screen.findByRole("dialog", { name: "October 2026 in review" });
  expect(dialog.querySelector("td")?.textContent).toBe("Dining out");
  expect(dialog.textContent).toContain("Household");
  // Once it's there, the keyboard is on its first control, and Tab stays inside it.
  const print = within(dialog).getByRole("button", { name: "Print or save as PDF" });
  const shut = within(dialog).getByRole("button", { name: "Close the preview" });
  await waitFor(() => expect(document.activeElement).toBe(print));
  fireEvent.keyDown(print, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(shut);
  fireEvent.keyDown(shut, { key: "Tab" });
  expect(document.activeElement).toBe(print);
  const before = document.title;
  // The card and its preview both say that printing is how a PDF gets saved.
  expect(screen.getAllByRole("button", { name: "Print or save as PDF" })).toHaveLength(2);
  fireEvent.click(within(dialog).getByRole("button", { name: "Print or save as PDF" }));
  await waitFor(() => expect(assistant.print).toHaveBeenCalled());
  // While the dialog is open, the page holds only the report, named for the PDF.
  expect(document.body.classList.contains("printing-report")).toBe(true);
  expect(document.querySelector(".report-print-root h1")?.textContent).toBe("October 2026 in review");
  expect(document.title).toBe("October 2026 in review");
  act(() => {
    window.dispatchEvent(new Event("afterprint"));
  });
  expect(document.body.classList.contains("printing-report")).toBe(false);
  expect(document.querySelector(".report-print-root")).toBeNull();
  expect(document.title).toBe(before);
  // Its table can be saved as a spreadsheet too.
  vi.mocked(assistant.spreadsheet).mockResolvedValue("C:\\data\\exports\\October 2026 in review.xlsx");
  fireEvent.click(screen.getByRole("button", { name: "Excel" }));
  expect(await screen.findByText(/Saved “October 2026 in review.xlsx”/)).toBeTruthy();
  expect(vi.mocked(assistant.spreadsheet).mock.calls[0].slice(1)).toEqual(["t_3", "p_r", false]);
});

it("opens or saves a spreadsheet the assistant made", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_4");
  const sheet: Proposal = {
    id: "p_s",
    title: "Groceries since June",
    kind: "spreadsheet",
    status: "proposed",
    made: true,
    actions: ["open", "save", "remove"],
    rows: 0,
    sheets: 2,
    card: {
      kicker: "Spreadsheet",
      rows: [
        { label: "Groceries", detail: "Transactions: Groceries, since Jun 1, 2026", badge: "214 rows" },
        { label: "By month", detail: "Spending by category, Jun – Oct 2026", badge: "1 category" },
      ],
      stats: [{ label: "Sheets", text: "2" }],
      more: 0,
      highlight: { categories: [], transactions: [] },
    },
  };
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_4", title: "Sheet", running: false, entries: [{ kind: "proposal", proposal: sheet }] });
  vi.mocked(assistant.spreadsheet).mockResolvedValue("/home/pat/Downloads/Groceries since June.xlsx");
  panel();
  const card = await screen.findByLabelText("Proposal: Groceries since June");
  expect(card.textContent).toContain("214 rows");
  fireEvent.click(within(card).getByRole("button", { name: "Open" }));
  await waitFor(() => expect(assistant.spreadsheet).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.spreadsheet).mock.calls[0].slice(1)).toEqual(["t_4", "p_s", true]);
  // Opening needs no message, and neither does a save panel that was cancelled.
  expect(screen.queryByText(/Saved/)).toBeNull();
  vi.mocked(assistant.spreadsheet).mockResolvedValueOnce(null);
  fireEvent.click(within(card).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(assistant.spreadsheet).toHaveBeenCalledTimes(2));
  expect(screen.queryByText(/Saved/)).toBeNull();
  // Saving says which file it was.
  fireEvent.click(within(card).getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Saved “Groceries since June.xlsx”.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Show in folder" })).toBeTruthy();
});

it("prints a report from its own frame on Chromium, leaving the page alone", () => {
  const before = document.title;
  void printReport(
    {
      title: "Spending <October>",
      period: "October 2026",
      body: "| Category | Spent |\n|---|---:|\n| Dining out | 1,240.00 |",
      created: "2026-10-16T10:00:00Z",
      budget: "Household",
    },
    "frame",
  );
  const frame = document.querySelector<HTMLIFrameElement>("iframe.report-frame")!;
  expect(frame.srcdoc).toContain("<title>Spending &lt;October&gt;</title>");
  expect(frame.srcdoc).toContain(">Dining out</td>");
  expect(document.title).toBe("Spending <October>");
  expect(document.body.classList.contains("printing-report")).toBe(false);
  expect(document.querySelector(".report-print-root")).toBeNull();
  frame.remove();
  document.title = before;
});

it("keeps a problem in the assistant inside the assistant", () => {
  const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
  let broken = true;
  const Panel = () => {
    if (broken) throw new Error("Cannot read properties of undefined");
    return <p>Working again</p>;
  };
  render(
    <AssistantBoundary budgetId="b1" onClose={() => {}}>
      <Panel />
    </AssistantBoundary>,
  );
  expect(screen.getByRole("alert").textContent).toContain("The assistant ran into a problem");
  expect(screen.getByText(/Cannot read properties of undefined/)).toBeTruthy();
  broken = false;
  fireEvent.click(screen.getByRole("button", { name: "Reload the assistant" }));
  expect(screen.getByText("Working again")).toBeTruthy();
  quiet.mockRestore();
});

it("reads what the model is working on", () => {
  expect(thinkingTitle("**Checking the month**\n\nI'm looking at categories")).toBe("Checking the month");
  expect(thinkingTitle("**First**\n\nx\n\n**Then the report**\n\ny")).toBe("Then the report");
  expect(thinkingTitle("I am checking the month. Now the rep")).toBe("I am checking the month.");
  expect(thinkingTitle("Starting")).toBe("");
});

it("renders lists after a sentence, tables and code, and never links or HTML", () => {
  const { container } = render(
    <Markdown
      text={[
        "Here is where it went:",
        "- **Rent** 5,000.00",
        "- Food 1,262.35",
        "",
        "| Category | Spent |",
        "|---|---:|",
        "| Rent | 5,000.00 |",
        "",
        "See [the report](https://example.com) and `code`.",
      ].join("\n")}
    />,
  );
  expect(container.querySelector("p")?.textContent).toBe("Here is where it went:");
  expect(container.querySelectorAll("ul li")).toHaveLength(2);
  // Columns are aligned by the side a row starts and ends on, so a table reads right in Arabic too.
  expect(container.querySelector("td:first-child")?.getAttribute("style")).toContain("start");
  expect(container.querySelector("td:last-child")?.getAttribute("style")).toContain("end");
  // Each block takes its direction from its own text.
  expect(container.querySelector("p")?.getAttribute("dir")).toBe("auto");
  expect(container.querySelector("ul")?.getAttribute("dir")).toBe("auto");
  expect(container.querySelector("table")?.getAttribute("dir")).toBe("auto");
  expect(container.querySelector("a")).toBeNull();
  expect(container.querySelector(".md-link")?.textContent).toBe("the report");
  expect(container.querySelector("code")?.textContent).toBe("code");
});

it("closes an unfinished bold run while streaming", () => {
  const { container } = render(<Markdown text={"Spent **240"} streaming />);
  expect(container.querySelector("strong")?.textContent).toBe("240");
  expect(container.textContent).not.toContain("*");
  const { container: early } = render(<Markdown text={"Total: **"} streaming />);
  expect(early.textContent).toBe("Total: ");
});

it("renders a little Markdown and never HTML", () => {
  const { container } = render(<Markdown text={"<b>raw</b> and **bold**\n\n- one\n- two\n\n1. first"} />);
  expect(container.querySelector("b")).toBeNull();
  expect(container.textContent).toContain("<b>raw</b>");
  expect(container.querySelector("strong")?.textContent).toBe("bold");
  expect(container.querySelectorAll("ul li")).toHaveLength(2);
  expect(container.querySelectorAll("ol li")).toHaveLength(1);
});

it("suggests what fits the budget", () => {
  const b = demoBudget();
  const ideas = suggestions(b, summary, { page: "budget", month: "2026-10" });
  expect(ideas[0]).toBe("Cover the overspent category");
  expect(ideas.at(-1)).toBe("How is October 2026 going?");
});

it("puts an import's request and transactions in the box without sending them", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  // A field's text is as tall as it is long here: two lines of it, or one of a placeholder.
  const tall = vi.spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLTextAreaElement) {
    return this.value ? 44 : 22;
  });
  panel({ request: { key: 1, text: "Categorize the 2 new transactions in Checking.", selected: ["bank:a", "bank:b"] } });
  const box = (await screen.findByLabelText("Message the assistant")) as HTMLTextAreaElement;
  await waitFor(() => expect(box.value).toBe("Categorize the 2 new transactions in Checking."));
  expect(screen.getByText("2 selected transactions")).toBeTruthy();
  expect(assistant.send).not.toHaveBeenCalled();
  // The box opens to the whole request, not a line of it.
  expect(box.style.height).toBe("44px");
  fireEvent.change(box, { target: { value: "" } });
  expect(box.style.height).toBe("22px");
  tall.mockRestore();
});

const hello = { kind: "user" as const, text: "Hi there", attachments: [] };
/** Opens the panel on a conversation it remembers, with these entries. */
function remembered(view: Partial<Awaited<ReturnType<typeof assistant.thread>>> = {}) {
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello], ...view });
}
/** Sends a message from the box and waits for the conversation it went to. */
async function say(text: string) {
  const box = await messageBox();
  fireEvent.change(box, { target: { value: text } });
  fireEvent.keyDown(box, { key: "Enter" });
  await waitFor(() => expect(assistant.send).toHaveBeenCalled());
}
const count = (n: number) => n.toLocaleString();

it("says what the assistant is doing while no text arrives, and stops saying it when something does", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Dining", entries: [hello], running: true });
  panel();
  await say("Hi there");
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith(expect.anything(), "t_1"));
  act(() => emit({ threadId: "t_1", type: "started" }));
  expect(await screen.findByText("Thinking")).toBeTruthy();
  act(() => emit({ threadId: "t_1", type: "activity", text: "Looking at the month" }));
  expect(screen.getByText("Looking at the month…")).toBeTruthy();
  expect(screen.queryByText("Thinking")).toBeNull();
  // The lookup's own line takes its place once it's done.
  act(() => emit({ threadId: "t_1", type: "step", text: "Checked October 2026", failed: false }));
  expect(screen.queryByText("Looking at the month…")).toBeNull();
  expect(screen.getByText("Thinking")).toBeTruthy();
  // Text arriving is the reply itself: nothing else is said to be going on.
  act(() => {
    emit({ threadId: "t_1", type: "activity", text: "Looking up transactions" });
    emit({ threadId: "t_1", type: "delta", text: "Dining out" });
  });
  expect(screen.queryByText("Looking up transactions…")).toBeNull();
  expect(await screen.findByText("Dining out")).toBeTruthy();
  // Another conversation's doings aren't this one's.
  act(() => emit({ threadId: "t_other", type: "activity", text: "Reading the file" }));
  expect(screen.queryByText("Reading the file…")).toBeNull();
});

it("says what goes with a message and shows it on the message once sent", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  const budget = demoBudget();
  const account = budget.accounts[0];
  vi.mocked(assistant.thread).mockResolvedValue({
    id: "t_1",
    title: "Odd",
    running: true,
    entries: [
      { kind: "user", text: "Anything odd here?", attachments: [], with: { page: "transactions", month: "2026-10", account: account.name, transactions: 2, import: false } },
    ],
  });
  panel({ budget, screen: { page: "transactions", month: "2026-10", accountId: account.id }, request: { key: 1, selected: ["t1", "t2"] } });
  const line = await screen.findByRole("group", { name: "With this message" });
  // The page and the account in view; a month only where the page shows one.
  expect(line.textContent).toContain(`Transactions · ${account.name}`);
  expect(line.textContent).not.toContain("October");
  expect(within(line).getByText("2 selected transactions")).toBeTruthy();
  await say("Anything odd here?");
  expect(vi.mocked(assistant.send).mock.calls[0][4]).toMatchObject({ page: "transactions", month: "2026-10", accountId: account.id, selected: ["t1", "t2"] });
  // The same line, small, on the bubble; and the selection has gone with it.
  const bubble = (await screen.findByText("Anything odd here?")).closest(".assistant-user") as HTMLElement;
  await waitFor(() => expect(bubble.querySelector(".assistant-sent-with")?.textContent).toBe(`Transactions · ${account.name} · 2 selected transactions`));
  expect(within(screen.getByRole("group", { name: "With this message" })).queryByText("2 selected transactions")).toBeNull();
});

it("names the category selected on the Budget page, and nothing for a message sent before this was kept", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered();
  const budget = demoBudget();
  const category = budget.categories[0];
  panel({ budget, screen: { page: "budget", month: "2026-10", categoryId: category.id } });
  expect((await screen.findByRole("group", { name: "With this message" })).textContent).toContain(`Budget · October 2026 · ${category.name}`);
  expect((await screen.findByText("Hi there")).closest(".assistant-user")?.querySelector(".assistant-sent-with")).toBeNull();
  expect(place("budget", "2026-10")).toEqual(["Budget", "October 2026"]);
  expect(place("reports", "2026-10")).toEqual(["Reports"]);
  expect(place("", "2026-10", "Checking")).toEqual([]);
});

it("doesn't send a part of what goes with a message once the user takes it off, for that message only", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Hi", entries: [hello], running: false });
  const file = { account: "checking", content: "date,amount", session: {} as never };
  panel({ screen: { page: "import", month: "2026-10", import: file } });
  const line = await screen.findByRole("group", { name: "With this message" });
  expect(line.textContent).toContain("Import a bank file");
  expect(within(line).getByText("The file being imported")).toBeTruthy();
  // Each part has its own ×.
  fireEvent.click(within(line).getByRole("button", { name: "Remove The file being imported" }));
  expect(screen.queryByText("The file being imported")).toBeNull();
  fireEvent.click(within(line).getByRole("button", { name: "Remove Import a bank file" }));
  expect(screen.queryByRole("group", { name: "With this message" })).toBeNull();
  await say("Hi there");
  const sent = vi.mocked(assistant.send).mock.calls[0][4];
  // Where the user is, taken off: no page, and no month, account or category with it.
  expect(sent.page).toBe("");
  expect(sent).not.toHaveProperty("month");
  expect(sent).not.toHaveProperty("accountId");
  expect(sent.import).toBeUndefined();
  // The next message starts with all of it again.
  await waitFor(() => expect(screen.getByRole("group", { name: "With this message" }).textContent).toContain("Import a bank file"));
  expect(screen.getByText("The file being imported")).toBeTruthy();
});

it("attaches the first 200 of a longer selection, and says how many were selected", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_1");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_1", title: "Hi", entries: [], running: true });
  const ids = Array.from({ length: 1234 }, (_, i) => `t${i}`);
  panel({ request: { key: 1, selected: ids } });
  // The chip says how many are attached, and the sentence how many there were.
  expect(await screen.findByText("200 selected transactions")).toBeTruthy();
  expect(screen.getByText(`Only the first 200 of ${count(1234)} selected transactions are attached.`)).toBeTruthy();
  await say("Categorize these");
  expect(vi.mocked(assistant.send).mock.calls[0][4].selected).toEqual(ids.slice(0, 200));
  await waitFor(() => expect(screen.queryByText(/Only the first 200/)).toBeNull());
});

it("offers the rest of a reply that was cut short, where the conversation ends", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.retry).mockResolvedValue(undefined);
  remembered({
    entries: [hello, { kind: "assistant", text: "The first part", index: 1 }, { kind: "cut" }, { kind: "assistant", text: "The second part", index: 3 }, { kind: "cut" }],
  });
  panel();
  await screen.findByText("The second part");
  expect(screen.getAllByText("The reply was cut short.")).toHaveLength(2);
  // Only the one the conversation ends on can be carried on.
  const go = screen.getAllByRole("button", { name: "Continue" });
  expect(go).toHaveLength(1);
  fireEvent.click(go[0]);
  await waitFor(() => expect(assistant.retry).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.retry).mock.calls[0][1]).toBe("t_9");
});

it("marks a reply cut short as it arrives", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true });
  panel();
  await screen.findByText("Hi there");
  act(() => {
    emit({ threadId: "t_9", type: "message", text: "As far as it got" });
    emit({ threadId: "t_9", type: "cut" });
  });
  expect(screen.getByText("The reply was cut short.")).toBeTruthy();
  // Still arriving as far as the panel knows: nothing to press yet.
  expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
});

it("offers to carry a long conversation on in a new one, and opens it", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockImplementation(async (_budget, id) =>
    id === "t_9"
      ? { id, title: "October so far", running: false, long: true, entries: [hello, { kind: "assistant", text: "Hello", index: 1 }] }
      : { id, title: "October so far", running: false, long: false, entries: [], continuedFrom: "October so far" },
  );
  vi.mocked(assistant.continue).mockResolvedValue("t_10");
  panel();
  expect(await screen.findByText("This conversation is getting long, so replies are slower and use more of your plan or key.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Continue in a new conversation" }));
  await waitFor(() => expect(assistant.continue).toHaveBeenCalledWith("t_9"));
  // The new one is open, remembered, and says what it carries on.
  expect(await screen.findByText("Continued from “October so far”")).toBeTruthy();
  expect(assistant.thread).toHaveBeenLastCalledWith(expect.anything(), "t_10");
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBe("t_10");
  expect(screen.queryByText(/getting long/)).toBeNull();
  expect(screen.queryByText("Hi there")).toBeNull();
});

it("offers the same under an error that says the conversation no longer fits", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ entries: [hello, { kind: "error", text: "This conversation is too long for the model.", code: "full", action: "newConversation" }] });
  vi.mocked(assistant.continue).mockResolvedValue("t_10");
  panel();
  const problem = (await screen.findByText("This conversation is too long for the model.")).closest(".assistant-problem") as HTMLElement;
  fireEvent.click(within(problem).getByRole("button", { name: "Continue in a new conversation" }));
  await waitFor(() => expect(assistant.continue).toHaveBeenCalledWith("t_9"));
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
});

it("shows the open conversation's title, and renames it", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ title: "Groceries since June" });
  const onClose = vi.fn();
  panel({ onClose });
  const title = await screen.findByText("Groceries since June");
  // Cut short where the header runs out, with the whole of it a hover away.
  expect(title.getAttribute("title")).toBe("Groceries since June");
  const pencil = screen.getByRole("button", { name: "Rename conversation" });
  fireEvent.click(pencil);
  let field = screen.getByLabelText("Conversation name") as HTMLInputElement;
  expect(field.value).toBe("Groceries since June");
  expect(document.activeElement).toBe(field);
  // Esc leaves the title as it was, and closes only the field.
  fireEvent.change(field, { target: { value: "Something else" } });
  fireEvent.keyDown(field, { key: "Escape" });
  expect(screen.queryByLabelText("Conversation name")).toBeNull();
  expect(assistant.renameThread).not.toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Rename conversation" }));
  // Enter keeps what was typed.
  fireEvent.click(screen.getByRole("button", { name: "Rename conversation" }));
  field = screen.getByLabelText("Conversation name") as HTMLInputElement;
  fireEvent.change(field, { target: { value: "  Food this summer " } });
  fireEvent.submit(field.closest("form")!);
  await waitFor(() => expect(assistant.renameThread).toHaveBeenCalledWith("t_9", "Food this summer"));
  expect(await screen.findByText("Food this summer")).toBeTruthy();
  expect(screen.queryByLabelText("Conversation name")).toBeNull();
});

it("lists every conversation with its date, finds one by its title, and opens it", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  const all = Array.from({ length: 20 }, (_, i) => ({ id: `t_${i}`, title: i === 9 ? "Rent" : i === 17 ? "Groceries since June" : `Conversation ${i}`, updated: "2026-10-16T10:00:00Z" }));
  vi.mocked(assistant.threads).mockResolvedValue(all);
  remembered({ title: "Rent", usage: { input: 12345, output: 678, cached: 10000 } });
  panel();
  await screen.findByText("Hi there");
  fireEvent.click(screen.getByRole("button", { name: "Conversations" }));
  const search = await screen.findByLabelText("Search conversations");
  // All of them, not the newest few; the open one is marked and says what it has used.
  await waitFor(() => expect(document.querySelectorAll(".assistant-history-item")).toHaveLength(20));
  const here = document.querySelector('.assistant-history-item[aria-current="true"]') as HTMLElement;
  expect(here.textContent).toContain("Rent");
  expect(here.textContent).toContain("2026");
  expect(here.textContent).toContain(`Tokens: ${count(12345)} sent (${count(10000)} from cache), ${count(678)} received`);
  fireEvent.change(search, { target: { value: "zzz" } });
  expect(screen.getByText("No conversation matches.")).toBeTruthy();
  fireEvent.change(search, { target: { value: " grocer" } });
  expect(document.querySelectorAll(".assistant-history-item")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /Groceries since June/ }));
  await waitFor(() => expect(assistant.thread).toHaveBeenLastCalledWith(expect.anything(), "t_17"));
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBe("t_17");
  expect(screen.queryByLabelText("Search conversations")).toBeNull();
});

it("says when the conversations can't be listed, instead of that there are none", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.threads).mockRejectedValueOnce("The folder can't be read.").mockResolvedValue([]);
  panel();
  await messageBox();
  fireEvent.click(screen.getByRole("button", { name: "Conversations" }));
  expect(await screen.findByText("The conversations couldn't be loaded.")).toBeTruthy();
  expect(screen.queryByText("No earlier conversations")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("No earlier conversations")).toBeTruthy();
});

it("asks before deleting a conversation", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.threads).mockResolvedValue([{ id: "t_9", title: "Rent", updated: "2026-10-16T10:00:00Z" }]);
  remembered({ title: "Rent" });
  panel();
  await screen.findByText("Hi there");
  const ask = async () => {
    fireEvent.click(screen.getByRole("button", { name: "Conversations" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete this conversation" }));
    return screen.getByRole("dialog", { name: "Delete this conversation?" });
  };
  let dialog = await ask();
  expect(dialog.textContent).toContain("“Rent”");
  expect(dialog.textContent).toContain("can't be undone");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog", { name: "Delete this conversation?" })).toBeNull();
  expect(assistant.deleteThread).not.toHaveBeenCalled();
  expect(screen.getByText("Hi there")).toBeTruthy();
  dialog = await ask();
  fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
  expect(assistant.deleteThread).toHaveBeenCalledWith("t_9");
  expect(screen.queryByText("Hi there")).toBeNull();
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBeNull();
});

it("keeps the user's thumb on a reply, and takes it off when it's pressed again", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ entries: [hello, { kind: "assistant", text: "You spent 4,070.00.", index: 3, tools: ["get_report"] }] });
  panel();
  const thumb = (name: string) => screen.getByRole("button", { name });
  expect((await screen.findByRole("button", { name: "Good reply" })).getAttribute("aria-pressed")).toBe("false");
  fireEvent.click(thumb("Good reply"));
  await waitFor(() => expect(thumb("Good reply").getAttribute("aria-pressed")).toBe("true"));
  expect(assistant.rate).toHaveBeenLastCalledWith("t_9", 3, true);
  fireEvent.click(thumb("Good reply"));
  await waitFor(() => expect(thumb("Good reply").getAttribute("aria-pressed")).toBe("false"));
  expect(assistant.rate).toHaveBeenLastCalledWith("t_9", 3, null);
  fireEvent.click(thumb("Bad reply"));
  await waitFor(() => expect(thumb("Bad reply").getAttribute("aria-pressed")).toBe("true"));
  expect(assistant.rate).toHaveBeenLastCalledWith("t_9", 3, false);
  expect(thumb("Good reply").getAttribute("aria-pressed")).toBe("false");
});

it("starts a report with the details of where the reply came from, for the user to keep or clear", async () => {
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, provider: "anthropic", model: "claude-sonnet" });
  vi.mocked(assistant.reportReply).mockResolvedValue("rep_1");
  remembered({ entries: [hello, { kind: "assistant", text: "You spent 4,070.00.", index: 3, tools: ["get_month", "get_report"] }] });
  panel();
  fireEvent.click(await screen.findByRole("button", { name: "Report a problem" }));
  const details = screen.getByLabelText("Details to include") as HTMLTextAreaElement;
  // No version here: there's no native side to ask in a test.
  expect(details.value).toBe("App: UrBudget desktop · Service: Anthropic · Model: claude-sonnet · Tools: get_month, get_report · Language: en");
  fireEvent.change(screen.getByLabelText("What's wrong? (optional)"), { target: { value: "The total is wrong." } });
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  await waitFor(() => expect(assistant.reportReply).toHaveBeenCalledWith("You spent 4,070.00.", `The total is wrong.\n\n${details.value}`));
});

it("waits quietly for a remembered conversation, offering nothing that would send into it", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  let arrive: (view: Awaited<ReturnType<typeof assistant.thread>>) => void = () => {};
  vi.mocked(assistant.thread).mockReturnValue(new Promise((resolve) => (arrive = resolve)));
  panel();
  await messageBox();
  expect(screen.getByText("Loading…")).toBeTruthy();
  expect(screen.queryByText("What would you like to do?")).toBeNull();
  expect(screen.queryByRole("button", { name: "Cover the overspent category" })).toBeNull();
  await act(async () => arrive({ id: "t_9", title: "Hi", running: false, entries: [hello] }));
  expect(await screen.findByText("Hi there")).toBeTruthy();
  expect(screen.queryByText("Loading…")).toBeNull();
});

it("says when a conversation can't be loaded, and keeps it to try again", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  localStorage.setItem("lb:assistant:thread:b1", "t_9");
  vi.mocked(assistant.thread).mockRejectedValueOnce("The file can't be read.").mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello] });
  panel();
  expect(await screen.findByText("This conversation couldn't be loaded.")).toBeTruthy();
  // Not forgotten, and nothing offered that would start another in its place unasked.
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBe("t_9");
  expect(screen.queryByText("What would you like to do?")).toBeNull();
  expect(screen.getAllByRole("button", { name: "New conversation" })).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Hi there")).toBeTruthy();
  expect(screen.queryByText("This conversation couldn't be loaded.")).toBeNull();
});

it("says when the assistant can't be loaded, and tries again", async () => {
  vi.mocked(assistant.status).mockRejectedValueOnce("The settings can't be read.").mockResolvedValue(ready);
  panel();
  expect(screen.getByText("Checking the assistant…")).toBeTruthy();
  expect(await screen.findByText("The assistant couldn't be loaded.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await messageBox()).toBeTruthy();
  expect(screen.queryByText("The assistant couldn't be loaded.")).toBeNull();
});

it("offers the one thing that helps over the field of a conversation that can't go on", async () => {
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, ready: false, account: null });
  vi.mocked(assistant.signIn).mockResolvedValue(ready);
  remembered();
  const handlers = panel();
  // The conversation is still there to read, not replaced by the sign-in screen.
  expect(await screen.findByText("Hi there")).toBeTruthy();
  const banner = screen.getByText("Sign in to use the assistant").closest(".assistant-banner") as HTMLElement;
  expect(screen.getByRole("button", { name: "Send" })).toHaveProperty("disabled", true);
  fireEvent.click(within(banner).getByRole("button", { name: "Continue with ChatGPT" }));
  await waitFor(() => expect(screen.queryByText("Sign in to use the assistant")).toBeNull());
  expect(assistant.signIn).toHaveBeenCalledTimes(1);
  // Signing in is all it does: nothing is asked again by itself.
  expect(assistant.retry).not.toHaveBeenCalled();
  // With a key gone, it's the assistant's settings.
  act(() => tell({ ...ready, ready: false, provider: "openai", account: null }));
  fireEvent.click(within(screen.getByText(/your own OpenAI API key/).closest(".assistant-banner") as HTMLElement).getByRole("button", { name: "Open settings" }));
  expect(handlers.onOpenSettings).toHaveBeenCalledTimes(1);
});

it("starts a new conversation for something asked from elsewhere, unless a reply is still arriving", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered();
  const view = render(<Hosted />);
  await screen.findByText("Hi there");
  view.rerender(<Hosted ask={{ key: 5, text: "Help me import this statement." }} />);
  const box = await messageBox();
  await waitFor(() => expect(box.value).toBe("Help me import this statement."));
  // Its own subject: the conversation that was open is left as it was.
  expect(screen.queryByText("Hi there")).toBeNull();
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBeNull();
  expect(assistant.send).not.toHaveBeenCalled();
  cleanup();
  forgetComposers();
  // While a reply is arriving, it waits in the field of the conversation it arrives in.
  remembered({ running: true });
  const busy = render(<Hosted />);
  await screen.findByText("Hi there");
  busy.rerender(<Hosted ask={{ key: 6, text: "Is rent paid?" }} />);
  await waitFor(() => expect((screen.getByLabelText("Message the assistant") as HTMLTextAreaElement).value).toBe("Is rent paid?"));
  expect(screen.getByText("Hi there")).toBeTruthy();
  expect(localStorage.getItem("lb:assistant:thread:b1")).toBe("t_9");
});

it("can start a new conversation while a reply is arriving in another, as it can open one", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true });
  panel();
  await screen.findByText("Hi there");
  expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "New conversation" }));
  expect(screen.queryByText("Hi there")).toBeNull();
  expect(screen.getByText("What would you like to do?")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
  // What the reply left behind goes on saying isn't shown here.
  act(() => emit({ threadId: "t_9", type: "delta", text: "elsewhere" }));
  expect(screen.queryByText("elsewhere")).toBeNull();
});

it("closes on Esc, except while a reply is arriving", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true });
  const onClose = vi.fn();
  panel({ onClose });
  await screen.findByText("Hi there");
  const box = await messageBox();
  // Stopping a reply is a button, not a key pressed in passing.
  fireEvent.keyDown(box, { key: "Escape" });
  expect(onClose).not.toHaveBeenCalled();
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello] });
  act(() => emit({ threadId: "t_9", type: "done" }));
  await screen.findByRole("button", { name: "Send" });
  fireEvent.keyDown(box, { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("answers a notice with Esc before closing the panel", async () => {
  let now: Status = { ...ready, planNoticeSeen: false };
  vi.mocked(assistant.status).mockImplementation(async () => now);
  vi.mocked(assistant.settings).mockImplementation(async (change) => (now = { ...now, planNoticeSeen: change.planNoticeSeen ?? now.planNoticeSeen }));
  const onClose = vi.fn();
  panel({ onClose });
  const notice = await screen.findByRole("dialog", { name: "You're using your ChatGPT plan" });
  fireEvent.keyDown(within(notice).getByRole("button", { name: "Got it" }), { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(assistant.settings).toHaveBeenCalledWith({ planNoticeSeen: true });
  expect(onClose).not.toHaveBeenCalled();
  fireEvent.keyDown(await messageBox(), { key: "Escape" });
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("takes the keyboard to the message field when the user opens it, and not otherwise", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  // Open from last time, as the app starts: the keyboard stays where it is.
  panel();
  const untouched = await messageBox();
  expect(document.activeElement).not.toBe(untouched);
  cleanup();
  panel({ autoFocus: true });
  const box = await messageBox();
  await waitFor(() => expect(document.activeElement).toBe(box));
});

it("reads aloud that a reply finished or a card arrived, never the text as it streams", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true });
  panel();
  await screen.findByText("Hi there");
  const said = () => document.querySelector('.assistant [aria-live="polite"]')?.textContent;
  // The conversation itself isn't a live region, and the row that says what's going on isn't one either.
  expect(document.querySelector(".assistant-feed")?.hasAttribute("aria-live")).toBe(false);
  expect(document.querySelectorAll(".assistant [aria-live], .assistant [role=status]")).toHaveLength(1);
  act(() => {
    emit({ threadId: "t_9", type: "started" });
    emit({ threadId: "t_9", type: "delta", text: "Dining out is over." });
  });
  expect(said()).toBe("");
  act(() => {
    emit({ threadId: "t_9", type: "message", text: "Dining out is over." });
    emit({ threadId: "t_9", type: "proposal", proposal });
  });
  expect(said()).toBe("A proposal is ready to review.");
  act(() => emit({ threadId: "t_9", type: "done" }));
  expect(said()).toBe("The assistant replied.");
  // The next reply is said again: what was said is cleared as it starts.
  act(() => emit({ threadId: "t_9", type: "started" }));
  expect(said()).toBe("");
});

it("stops following the end on any move up, and follows again once back at the end", () => {
  const at = (top: number) => ({ top, height: 1000, view: 400 });
  // At the end: followed, however it got there.
  expect(following({ following: false, top: 300 }, at(600))).toBe(true);
  expect(following({ following: false, top: 300 }, at(599))).toBe(true);
  // Any move up stops it, even a small one.
  expect(following({ following: true, top: 600 }, at(590))).toBe(false);
  // A move down that stops short of the end changes nothing.
  expect(following({ following: false, top: 300 }, at(450))).toBe(false);
  expect(following({ following: true, top: 300 }, at(450))).toBe(true);
  // New text made it taller without the user moving: still followed.
  expect(following({ following: true, top: 600 }, { top: 600, height: 1200, view: 400 })).toBe(true);
  // What's in it got shorter (rows folded away) and the box was pushed up to its new end: followed.
  expect(following({ following: true, top: 600 }, { top: 400, height: 800, view: 400 })).toBe(true);
});

it("keeps room for the page beside the panel in a narrow window", () => {
  // Wide enough: as wide as it likes, up to its own limit.
  expect(widest(1600, 248)).toBe(640);
  // The page keeps 420 between the sidebar and the panel.
  expect(widest(1100, 248)).toBe(432);
  expect(widest(1100, 52)).toBe(628);
  // Never narrower than the panel can be.
  expect(widest(700, 248)).toBe(320);
});

const card = { kind: "proposal" as const, proposal };
/** The panel open on a remembered conversation that holds `p`, once its card is there. */
async function withCard(p: Proposal = proposal, props: Parameters<typeof panel>[0] = {}) {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ entries: [hello, { kind: "proposal", proposal: p }] });
  const handlers = panel(props);
  const shown = await screen.findByLabelText(`Proposal: ${p.title}`, {}, { timeout: 5000 });
  return { handlers, shown };
}

it("shows a reply's thumbs as soon as it has finished, and copies its text", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true });
  const writeText = vi.fn(async (_text: string) => {});
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  panel();
  await screen.findByText("Hi there");
  act(() => {
    emit({ threadId: "t_9", type: "started" });
    emit({ threadId: "t_9", type: "delta", text: "Dining out is **240.00** over." });
  });
  // While it arrives it has no place yet, and nothing to press.
  expect(screen.queryByRole("button", { name: "Good reply" })).toBeNull();
  // Its place comes with the finished reply: no waiting for the conversation to be read again.
  act(() => emit({ threadId: "t_9", type: "message", text: "Dining out is **240.00** over.", index: 4 }));
  const thumb = await screen.findByRole("button", { name: "Good reply" });
  await waitFor(() => expect(thumb).toHaveProperty("disabled", false));
  expect(assistant.thread).toHaveBeenCalledTimes(1);
  fireEvent.click(thumb);
  await waitFor(() => expect(assistant.rate).toHaveBeenCalledWith("t_9", 4, true));
  // Copy takes the reply as it was written, beside the thumbs and the report.
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  await waitFor(() => expect(writeText).toHaveBeenCalledWith("Dining out is **240.00** over."));
  expect(await screen.findByText("Copied.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Report a problem" })).toBeTruthy();
});

it("puts an earlier card's new view in its place when a newer one replaces it, and puts it back", async () => {
  await withCard();
  const newer: Proposal = { ...proposal, id: "p_2", title: "Cover dining and coffee" };
  act(() => {
    emit({ threadId: "t_9", type: "replaced", proposal: { ...proposal, status: "replaced", replacedBy: "p_2", actions: ["restore"] } });
    emit({ threadId: "t_9", type: "proposal", proposal: newer });
  });
  // In place, not a second card for it: the earlier one folded, then the newer one.
  expect(await screen.findByText("Replaced by a newer version")).toBeTruthy();
  const cards = [...document.querySelectorAll(".proposal")];
  expect(cards.map((c) => c.getAttribute("aria-label"))).toEqual(["Proposal: Cover dining", "Proposal: Cover dining and coffee"]);
  expect(cards[0].className).toContain("replaced");
  expect(screen.getAllByRole("button", { name: "Apply" })).toHaveLength(1);
  // Restore asks the core to put it back, and the card is what comes back.
  vi.mocked(assistant.mark).mockResolvedValue(proposal);
  fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  await waitFor(() => expect(assistant.mark).toHaveBeenCalledWith("t_9", "p_1", "proposed"));
  await waitFor(() => expect(screen.getAllByRole("button", { name: "Apply" })).toHaveLength(2));
  expect(screen.queryByText("Replaced by a newer version")).toBeNull();
});

it("sets a card aside, and reads the conversation again when a card can't do what was asked", async () => {
  await withCard();
  vi.mocked(assistant.mark).mockResolvedValueOnce({ ...proposal, status: "discarded", actions: ["restore"] });
  fireEvent.click(screen.getByRole("button", { name: "Discard" }));
  await waitFor(() => expect(assistant.mark).toHaveBeenCalledWith("t_9", "p_1", "discarded"));
  expect(await screen.findByText("Discarded")).toBeTruthy();
  // The core refuses a step that can't follow, or the proposal is gone: that is said, and the
  // card shows what the core has instead of a button that no longer works.
  vi.mocked(assistant.mark).mockRejectedValueOnce("That proposal is gone.");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello] });
  const reads = vi.mocked(assistant.thread).mock.calls.length;
  fireEvent.click(screen.getByRole("button", { name: "Restore" }));
  expect(await screen.findByText("That proposal is gone.")).toBeTruthy();
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledTimes(reads + 1));
  await waitFor(() => expect(document.querySelector(".proposal")).toBeNull());
});

it("shows a card the window changed outside the panel as it is now", async () => {
  await withCard();
  // The window's Undo took a change back, or an import was finished: whoever marked the card, the panel hears.
  act(() => marked("t_9", applied(proposal)));
  expect(await screen.findByText("Applied")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  // Another conversation's card isn't this one's.
  act(() => marked("t_other", proposal));
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  act(() => marked("t_9", { ...proposal, status: "undone" }));
  expect(await screen.findByText("Undone")).toBeTruthy();
});

it("sends the choices made on a card before applying it, and applies the card the core made of them", async () => {
  const choosable: Proposal = { ...proposal, card: { ...proposal.card, rows: [{ ...proposal.card.rows[0], key: "c:dining" }, { label: "Coffee", amount: 2000, key: "c:coffee" }] } };
  const { handlers } = await withCard(choosable);
  const revised: Proposal = { ...choosable, choices: { skip: ["c:coffee"], set: [] }, card: { ...choosable.card, rows: [choosable.card.rows[0], { ...choosable.card.rows[1], skipped: true }] } };
  vi.mocked(assistant.revise).mockResolvedValue(revised);
  fireEvent.click(screen.getByRole("checkbox", { name: "Apply: Coffee" }));
  fireEvent.click(screen.getByRole("button", { name: "Apply 1 of 2" }));
  await waitFor(() => expect(handlers.onApply).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.revise).mock.calls[0].slice(1)).toEqual(["t_9", "p_1", { skip: ["c:coffee"], set: [] }]);
  // Not the card as it was drawn when the button was pressed.
  expect(handlers.onApply).toHaveBeenCalledWith("t_9", revised);
});

it("says so when a card's choices can't be made, and shows the card as the core has it", async () => {
  const choosable: Proposal = { ...proposal, card: { ...proposal.card, rows: [{ ...proposal.card.rows[0], key: "c:dining" }, { label: "Coffee", amount: 2000, key: "c:coffee" }] } };
  const { handlers } = await withCard(choosable);
  vi.mocked(assistant.revise).mockRejectedValue("This proposal was already handled.");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello, { kind: "proposal", proposal: applied(choosable) }] });
  fireEvent.click(screen.getByRole("checkbox", { name: "Apply: Coffee" }));
  fireEvent.click(screen.getByRole("button", { name: "Apply 1 of 2" }));
  expect(await screen.findByText("This proposal was already handled.")).toBeTruthy();
  expect(await screen.findByText("Applied")).toBeTruthy();
  expect(handlers.onApply).not.toHaveBeenCalled();
});

it("puts a line about a card in the message field to ask for changes, and asks for a new version of one that failed", async () => {
  await withCard();
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "Also" } });
  fireEvent.click(screen.getByRole("button", { name: "Ask for changes" }));
  // For the user to finish, after whatever they were writing: nothing is sent.
  await waitFor(() => expect(box.value).toBe("Also\nRevise the proposal “Cover dining”: "));
  expect(assistant.send).not.toHaveBeenCalled();
  cleanup();
  forgetComposers();
  vi.mocked(assistant.send).mockResolvedValue("t_9");
  await withCard({ ...proposal, status: "failed", actions: ["askAgain", "discard"], message: { tone: "error", text: "Dining out no longer exists." } });
  expect(screen.getByText("Dining out no longer exists.").closest(".proposal-message")?.className).toContain("error");
  fireEvent.click(screen.getByRole("button", { name: "Ask for a new version" }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.send).mock.calls[0][2]).toBe("Prepare “Cover dining” again. It couldn't be applied: Dining out no longer exists.");
});

it("goes to what a card names in the budget", async () => {
  const { handlers } = await withCard();
  fireEvent.click(screen.getByRole("button", { name: "Show in the budget" }));
  expect(handlers.onShow).toHaveBeenCalledWith(proposal.card.highlight);
});

const question: Proposal = {
  id: "p_q",
  title: "Which account did you pay from?",
  kind: "question",
  status: "proposed",
  made: false,
  actions: ["answer"],
  rows: 0,
  sheets: 0,
  card: { kicker: "Question", rows: [{ label: "Everyday" }, { label: "Card" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
};

it("sends a question's answer as the user's message, through the same door as Send", async () => {
  vi.mocked(assistant.send).mockResolvedValue("t_9");
  await withCard(question);
  // A message being written isn't the answer, and isn't lost to it.
  const box = await messageBox();
  fireEvent.change(box, { target: { value: "half a thought" } });
  fireEvent.click(screen.getByRole("button", { name: "Card" }));
  await waitFor(() => expect(assistant.send).toHaveBeenCalledTimes(1));
  const [, thread, text, , where] = vi.mocked(assistant.send).mock.calls[0];
  expect([thread, text]).toEqual(["t_9", "Card"]);
  // What goes with a message goes with this one.
  expect(where).toMatchObject({ page: "budget", month: "2026-10" });
  expect(box.value).toBe("half a thought");
  cleanup();
  forgetComposers();
  vi.clearAllMocks();
  // Before a first message to a service, the notice of what gets sent comes first.
  vi.mocked(assistant.status).mockResolvedValue({ ...ready, noticeDue: true });
  remembered({ entries: [hello, { kind: "proposal", proposal: question }] });
  const handlers = panel();
  fireEvent.click(await screen.findByRole("button", { name: "Everyday" }));
  expect(await screen.findByRole("dialog", { name: "What gets sent" })).toBeTruthy();
  expect(assistant.send).not.toHaveBeenCalled();
  // A question isn't a change waiting: nothing of it is marked on the page.
  expect(handlers.onHighlight).not.toHaveBeenCalledWith(question.card.highlight);
});

it("keeps a question's answers from being pressed while a reply is still arriving", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ running: true, entries: [hello, { kind: "proposal", proposal: question }] });
  panel();
  expect(await screen.findByRole("button", { name: "Card" })).toHaveProperty("disabled", true);
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello, { kind: "proposal", proposal: question }] });
  act(() => emit({ threadId: "t_9", type: "done" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Card" })).toHaveProperty("disabled", false));
});

it("offers what's waiting in the budget under the last reply, and sends its message", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  vi.mocked(assistant.send).mockResolvedValue("t_9");
  const reply = { kind: "assistant" as const, text: "Dining out is covered.", index: 2 };
  const next = [
    { id: "cover", text: "Cover 2 overspent categories", prompt: "Cover my overspent categories this month." },
    { id: "categorize", text: "Categorize 3 transactions", prompt: "Categorize my transactions that have no category yet." },
  ];
  remembered({ entries: [hello, reply], next });
  panel();
  // The core's words, as buttons; what one sends is the core's too.
  const offered = await screen.findByRole("button", { name: "Cover 2 overspent categories" });
  expect(screen.getByRole("button", { name: "Categorize 3 transactions" })).toBeTruthy();
  expect(offered.closest(".assistant-next")?.previousElementSibling?.textContent).toContain("Dining out is covered.");
  vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: true, entries: [hello, reply, { kind: "user", text: next[0].prompt, attachments: [] }] });
  fireEvent.click(offered);
  await waitFor(() => expect(assistant.send).toHaveBeenCalledTimes(1));
  expect(vi.mocked(assistant.send).mock.calls[0][2]).toBe("Cover my overspent categories this month.");
  // While a reply is arriving there is nothing to offer.
  await waitFor(() => expect(screen.queryByRole("button", { name: "Cover 2 overspent categories" })).toBeNull());
  expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
});

it("keeps the budget's notes for the assistant in its settings: each added or removed as one change", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  // The core's change for a list of notes: the list set whole, each note once, or taken away when it's empty.
  vi.mocked(assistant.notes).mockImplementation(async (_budget, notes) => {
    const kept = [...new Set(notes)];
    return kept.length ? { extras: { set: { assistantNotes: kept }, delete: [] } } : { extras: { set: {}, delete: ["assistantNotes"] } };
  });
  const changes: (string[] | undefined)[] = [];
  function Settings() {
    const [budget, setBudget] = useState<Budget>(() => ({ ...demoBudget(), assistantNotes: ["Rent is due on the 1st"] }));
    return (
      <ToastProvider>
        <AssistantSettings
          budget={budget}
          transform={async (work) => {
            // One step of the window's own, as any other edit is.
            const next = await work(budget);
            changes.push(next.assistantNotes);
            setBudget(next);
          }}
        />
      </ToastProvider>
    );
  }
  render(<Settings />);
  expect(await screen.findByRole("heading", { name: "Notes for the assistant" }, { timeout: 5000 })).toBeTruthy();
  expect(screen.getByText("Rent is due on the 1st")).toBeTruthy();
  const field = screen.getByLabelText("Add a note") as HTMLInputElement;
  const add = screen.getByRole("button", { name: "Add a note" }) as HTMLButtonElement;
  expect(add.disabled).toBe(true);
  fireEvent.change(field, { target: { value: "  Savings come first " } });
  fireEvent.click(add);
  // The whole list goes to the core, and what it makes of it is the change.
  await waitFor(() => expect(changes).toEqual([["Rent is due on the 1st", "Savings come first"]]));
  expect(vi.mocked(assistant.notes).mock.calls[0][1]).toEqual(["Rent is due on the 1st", "Savings come first"]);
  expect(await screen.findByText("Savings come first")).toBeTruthy();
  await waitFor(() => expect(field.value).toBe(""));
  // Each note has its own way out.
  fireEvent.click(within(screen.getByText("Rent is due on the 1st").closest("li") as HTMLElement).getByRole("button", { name: "Remove note" }));
  await waitFor(() => expect(screen.queryByText("Rent is due on the 1st")).toBeNull());
  expect(vi.mocked(assistant.notes).mock.calls[1][1]).toEqual(["Savings come first"]);
  // A note that's already there changes nothing, so there is nothing to undo.
  fireEvent.change(field, { target: { value: "Savings come first" } });
  fireEvent.click(add);
  await waitFor(() => expect(assistant.notes).toHaveBeenCalledTimes(3));
  await waitFor(() => expect(field.value).toBe(""));
  expect(changes).toHaveLength(2);
  // What the core refuses is said, and what was typed stays to be changed.
  vi.mocked(assistant.notes).mockRejectedValueOnce("Keep at most 30 notes of up to 200 characters each.");
  fireEvent.change(field, { target: { value: "One too many" } });
  fireEvent.click(add);
  expect(await screen.findByText("Keep at most 30 notes of up to 200 characters each.")).toBeTruthy();
  expect(field.value).toBe("One too many");
  expect(changes).toHaveLength(2);
  // The last one removed, there are none, and the budget keeps no empty list.
  fireEvent.click(screen.getByRole("button", { name: "Remove note" }));
  expect(await screen.findByText("No notes yet.")).toBeTruthy();
  expect(changes[2]).toBeUndefined();
});

/** The panel with a budget that can be given again, as the window does when it changes. */
function Budgeted({ budget }: { budget: Budget }) {
  return (
    <ToastProvider>
      <AssistantPanel
        budget={budget}
        budgetId="b1"
        summary={summary}
        screen={{ page: "budget", month: "2026-10" }}
        request={null}
        width={380}
        onResize={() => {}}
        onClose={() => {}}
        onOpenSettings={() => {}}
        onApply={async (_t, p) => p}
        onUndo={async (_t, p) => p}
        canUndo={() => false}
        onReviewImport={() => {}}
        onHighlight={() => {}}
      />
    </ToastProvider>
  );
}

it("reads the conversation again half a second after the budget last changed, while a change waits", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  remembered({ entries: [hello, card] });
  const first = demoBudget();
  const view = render(<Budgeted budget={first} />);
  await screen.findByLabelText("Proposal: Cover dining", {}, { timeout: 5000 });
  expect(assistant.thread).toHaveBeenCalledTimes(1);
  expect(vi.mocked(assistant.thread).mock.calls[0][0]).toBe(first);
  // Only the wait is faked: the rest keeps real time.
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    const wait = (ms: number) =>
      act(async () => {
        vi.advanceTimersByTime(ms);
        for (let i = 0; i < 50; i++) await Promise.resolve();
      });
    // The card as the core makes it again on the budget as it is now, saying so.
    vi.mocked(assistant.thread).mockResolvedValue({
      id: "t_9",
      title: "Hi",
      running: false,
      entries: [hello, { kind: "proposal", proposal: { ...proposal, message: { tone: "info", text: "Your budget changed." } } }],
    });
    const second = { ...first, name: "Edited" };
    view.rerender(<Budgeted budget={second} />);
    await wait(REFRESH - 1);
    expect(assistant.thread).toHaveBeenCalledTimes(1);
    // Another edit meanwhile: the wait starts again, and one read does for both.
    const third = { ...second, name: "Edited again" };
    view.rerender(<Budgeted budget={third} />);
    await wait(REFRESH - 1);
    expect(assistant.thread).toHaveBeenCalledTimes(1);
    await wait(1);
    expect(assistant.thread).toHaveBeenCalledTimes(2);
    // With the window's budget as it is now, unsaved edits and all.
    expect(vi.mocked(assistant.thread).mock.calls[1]).toEqual([third, "t_9"]);
    expect(screen.getByText("Your budget changed.").closest(".proposal-message")?.className).toContain("info");
    // Drawn again without a budget change, nothing is read.
    view.rerender(<Budgeted budget={third} />);
    await wait(REFRESH);
    expect(assistant.thread).toHaveBeenCalledTimes(2);
    // With no change waiting, the budget changing is no reason to read it again.
    act(() => marked("t_9", applied(proposal)));
    view.rerender(<Budgeted budget={{ ...third, name: "And again" }} />);
    await wait(REFRESH);
    expect(assistant.thread).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});

it("keeps the next steps under the last reply up to date with the budget", async () => {
  vi.mocked(assistant.status).mockResolvedValue(ready);
  const reply = { kind: "assistant" as const, text: "Dining out is covered.", index: 2 };
  remembered({ entries: [hello, reply], next: [{ id: "cover", text: "Cover 1 overspent category", prompt: "Cover my overspent categories this month." }] });
  const first = demoBudget();
  const view = render(<Budgeted budget={first} />);
  await screen.findByRole("button", { name: "Cover 1 overspent category" }, { timeout: 5000 });
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    // Covered by hand meanwhile: counted again from the budget, there is nothing left to offer.
    vi.mocked(assistant.thread).mockResolvedValue({ id: "t_9", title: "Hi", running: false, entries: [hello, reply] });
    view.rerender(<Budgeted budget={{ ...first, name: "Edited" }} />);
    await act(async () => {
      vi.advanceTimersByTime(REFRESH);
      for (let i = 0; i < 50; i++) await Promise.resolve();
    });
    expect(assistant.thread).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: "Cover 1 overspent category" })).toBeNull();
  } finally {
    vi.useRealTimers();
  }
});
