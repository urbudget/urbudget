import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn(async (_command: string) => {}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
import { SendFeedbackRow } from "./Feedback";
import { ToastProvider } from "./ui";

afterEach(() => {
  cleanup();
  native.invoke.mockClear();
});

it("starts an email to support, and shows the address", () => {
  render(<ToastProvider><SendFeedbackRow /></ToastProvider>);
  expect(screen.getByText(/support@urbudget\.app/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Write an email" }));
  expect(native.invoke).toHaveBeenCalledWith("feedback_open");
});

it("says where to write when no mail app opens", async () => {
  native.invoke.mockRejectedValueOnce("");
  render(<ToastProvider><SendFeedbackRow /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", { name: "Write an email" }));
  expect(await screen.findByText("Couldn't open your mail app. Write to support@urbudget.app.")).toBeTruthy();
});
