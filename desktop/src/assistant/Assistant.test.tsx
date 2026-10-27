import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { AssistantEvent, Proposal, Status } from "./api";

let emit: (e: AssistantEvent) => void = () => {};
vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    assistant: {
      status: vi.fn(),
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
import { AssistantBoundary, AssistantPanel, Markdown, suggestions, thinkingTitle } from "./Assistant";
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

afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
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
      { kind: "error", text: "You've reached your ChatGPT plan's usage limit.", code: "limit" },
    ],
  });
  panel();
  expect(await screen.findByText("Usage limit reached")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Manage usage" }));
  expect(assistant.open).toHaveBeenCalledWith("usage");
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
  fireEvent.click(within(dialog).getByRole("button", { name: "Save as PDF" }));
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
