import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assistant } from "./api";
import { reasonWith, ReplyReport, reportDetails } from "./ReplyReport";

vi.mock("./api", () => ({ assistant: { reportReply: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());

it("sends nothing until the user explicitly submits the edited excerpt", async () => {
  vi.mocked(assistant.reportReply).mockResolvedValue("rep_reference");
  render(<ReplyReport text="Private balance: 1234. A harmful suggestion." close={vi.fn()} />);
  expect(assistant.reportReply).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("Reply to include"), { target: { value: " A harmful suggestion. " } });
  fireEvent.change(screen.getByLabelText("What's wrong? (optional)"), { target: { value: " Please review this. " } });
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  await waitFor(() => expect(assistant.reportReply).toHaveBeenCalledExactlyOnceWith("A harmful suggestion.", "Please review this."));
  expect(await screen.findByText("rep_reference")).toBeTruthy();
  expect(screen.queryByText(/Private balance/)).toBeNull();
});

it("lets the user cancel without sending any report", () => {
  const close = vi.fn();
  render(<ReplyReport text="reply" close={close} />);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(close).toHaveBeenCalled();
  expect(assistant.reportReply).not.toHaveBeenCalled();
});

it("preserves edits after a network failure and permits an explicit retry", async () => {
  vi.mocked(assistant.reportReply).mockRejectedValueOnce("You're offline.").mockResolvedValueOnce("rep_retried");
  render(<ReplyReport text="reply" close={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("What's wrong? (optional)"), { target: { value: "Unsafe advice" } });
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  expect((await screen.findByRole("alert")).textContent).toContain("You're offline.");
  expect((screen.getByLabelText("What's wrong? (optional)") as HTMLTextAreaElement).value).toBe("Unsafe advice");
  expect(assistant.reportReply).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  expect(await screen.findByText("rep_retried")).toBeTruthy();
  expect(assistant.reportReply).toHaveBeenCalledTimes(2);
});

it("limits the initial excerpt and prevents sending a blank report", () => {
  render(<ReplyReport text={"a".repeat(5000)} close={vi.fn()} />);
  const excerpt = screen.getByLabelText("Reply to include") as HTMLTextAreaElement;
  expect(excerpt.value).toHaveLength(4000);
  expect(excerpt.maxLength).toBe(4000);
  fireEvent.change(excerpt, { target: { value: " " } });
  expect((screen.getByRole("button", { name: "Send report" }) as HTMLButtonElement).disabled).toBe(true);
});

it("does not split an emoji when truncating the initial excerpt", () => {
  render(<ReplyReport text={`${"a".repeat(3999)}😀`} close={vi.fn()} />);
  expect((screen.getByLabelText("Reply to include") as HTMLTextAreaElement).value).toBe("a".repeat(3999));
});

const line = "App: UrBudget desktop 1.0.11 · Service: ChatGPT · Model: default · Tools: get_month · Language: en";

it("sends the details after the reason, as the user left them", async () => {
  vi.mocked(assistant.reportReply).mockResolvedValue("rep_details");
  render(<ReplyReport text="reply" details={line} close={vi.fn()} />);
  const details = screen.getByLabelText("Details to include") as HTMLTextAreaElement;
  expect(details.value).toBe(line);
  expect(screen.getByText("Sent with your report. Remove anything you'd rather not send.")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("What's wrong? (optional)"), { target: { value: " The total is wrong. " } });
  // Whatever the user takes out of them isn't sent.
  fireEvent.change(details, { target: { value: "App: UrBudget desktop 1.0.11 · Language: en" } });
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  await waitFor(() =>
    expect(assistant.reportReply).toHaveBeenCalledExactlyOnceWith("reply", "The total is wrong.\n\nApp: UrBudget desktop 1.0.11 · Language: en"),
  );
});

it("sends no details once they are cleared, and the details alone when nothing else is said", async () => {
  vi.mocked(assistant.reportReply).mockResolvedValue("rep_cleared");
  const view = render(<ReplyReport text="reply" details={line} close={vi.fn()} />);
  fireEvent.change(screen.getByLabelText("What's wrong? (optional)"), { target: { value: "Unsafe advice" } });
  fireEvent.change(screen.getByLabelText("Details to include"), { target: { value: "  " } });
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  await waitFor(() => expect(assistant.reportReply).toHaveBeenLastCalledWith("reply", "Unsafe advice"));
  view.unmount();
  render(<ReplyReport text="reply" details={line} close={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Send report" }));
  await waitFor(() => expect(assistant.reportReply).toHaveBeenLastCalledWith("reply", line));
});

it("fits the reason and its details in the reason's 1,000 characters by cutting the details, never the user's words", () => {
  expect(reasonWith("Wrong total", line)).toBe(`Wrong total\n\n${line}`);
  const long = "x".repeat(951);
  const sent = reasonWith(long, line);
  expect(sent).toHaveLength(1000);
  expect(sent).toBe(`${long}\n\n${line.slice(0, 47)}`);
  // No room for any of them after a blank line: the words go alone, whole.
  expect(reasonWith("x".repeat(999), line)).toBe("x".repeat(999));
  expect(reasonWith("x".repeat(1000), line)).toBe("x".repeat(1000));
  // A cut never leaves half an emoji.
  expect(reasonWith("x".repeat(997), "😀😀")).toBe("x".repeat(997));
  expect(reasonWith("x".repeat(995), "😀😀")).toBe(`${"x".repeat(995)}\n\n😀`);
});

it("writes the details line from the app, the service, the model, the reply's tools and the language", () => {
  expect(reportDetails({ version: "1.0.11", provider: "chatgpt", model: null, tools: [], language: "en" })).toBe(
    "App: UrBudget desktop 1.0.11 · Service: ChatGPT · Model: default · Tools: none · Language: en",
  );
  // A version that couldn't be read is left out, not made up.
  expect(reportDetails({ version: "", provider: "anthropic", model: "claude-sonnet", tools: ["get_month", "get_report"], language: "ar" })).toBe(
    "App: UrBudget desktop · Service: Anthropic · Model: claude-sonnet · Tools: get_month, get_report · Language: ar",
  );
  expect(reportDetails({ provider: "openai", language: "en" })).toContain("Service: OpenAI · Model: default · Tools: none");
});
