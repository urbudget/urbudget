import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { AssistantEvent, Proposal, Status } from "./api";

let emit: (e: AssistantEvent) => void = () => {};
/** A status the app was given somewhere else (Settings), as the panel hears of it. */
let tell: (s: Status) => void = () => {};
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
      attach: vi.fn(),
      detach: vi.fn(async () => {}),
      open: vi.fn(async () => {}),
      saveImport: vi.fn(),
      deleteThread: vi.fn(),
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
import { AssistantBoundary, AssistantPanel, Markdown, suggestions, thinkingTitle, type Request } from "./Assistant";
import { forgetComposers } from "./composer";
import { printReport } from "./Report";
import { demoBudget } from "../demo";
import { ToastProvider } from "../ui";
import type { Summary } from "../types";

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

function panel(props: Partial<Parameters<typeof AssistantPanel>[0]> = {}) {
  const handlers = {
    onApply: vi.fn(async (_t: string, p: Proposal) => ({ ...p, status: "applied" as const })),
    onUndo: vi.fn(),
    onHighlight: vi.fn(),
    onOpenSettings: vi.fn(),
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
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith("t_1"));
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
  expect(await screen.findByText("Applied: Cover dining")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Undo" })).toBeTruthy();
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
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith("t_1"));
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
  await waitFor(() => expect(assistant.thread).toHaveBeenCalledWith("t_1"));
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
  expect(screen.getByText("2 selected")).toBeTruthy();
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
  expect(container.querySelector("td:last-child")?.getAttribute("style")).toContain("right");
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

it("offers fixes for an import to apply there", async () => {
  const { ProposalCard } = await import("./ProposalCard");
  const onApply = vi.fn(async () => {});
  const fix = {
    id: "p1",
    title: "1 fix for the import",
    kind: "fix" as const,
    status: "proposed" as const,
    rows: 0,
    sheets: 0,
    card: { kicker: "Import fixes", rows: [{ label: "Line 30 · amount: n/a → -12.40", detail: "The statement's total" }], stats: [], more: 0, highlight: { categories: [], transactions: [] } },
  };
  render(
    <ProposalCard proposal={fix} canUndo={false} onApply={onApply} onDiscard={async () => {}} onUndo={async () => {}} onReview={() => {}} onPreview={() => {}} onSave={async () => {}} />,
  );
  expect(screen.getByText("The statement's total")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Apply to the import" }));
  await waitFor(() => expect(onApply).toHaveBeenCalledTimes(1));
  expect(screen.getByText("Each applies only to a line that hasn't changed")).toBeTruthy();
});
