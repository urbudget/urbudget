import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { coverAmount, MoveMoneyPopover } from "./MoneyPopovers";
import { AppliedChangeError } from "./useBudget";
import { demoBudget } from "./demo";
import { editableAmount } from "./format";
afterEach(cleanup);

it("covers overspending with all of it when the source has enough, else with everything the source has", () => {
  expect(coverAmount(12_000, 50_000)).toBe(12_000);
  expect(coverAmount(12_000, 12_000)).toBe(12_000);
  expect(coverAmount(12_000, 5_000)).toBe(5_000);
  // Nothing to give, or no source yet: the amount still says what's needed.
  expect(coverAmount(12_000, 0)).toBe(12_000);
  expect(coverAmount(12_000, -3_000)).toBe(12_000);
  expect(coverAmount(12_000, null)).toBe(12_000);
});

it("opens a cover from Unassigned with what Unassigned can give of the overspending", () => {
  const props = { budget: demoBudget(), anchor: { current: document.body }, source: "groceries", sourceName: "Groceries", available: -1000,
    balances: new Map<string, number>(), onMove: vi.fn(), onClose: vi.fn() };
  render(<MoveMoneyPopover {...props} readyToAssign={10000} />);
  expect((screen.getByLabelText("Amount") as HTMLInputElement).value).toBe(editableAmount(1000));
  cleanup();
  render(<MoveMoneyPopover {...props} readyToAssign={400} />);
  expect((screen.getByLabelText("Amount") as HTMLInputElement).value).toBe(editableAmount(400));
});

it("offers save-only retries after a move was already applied, keeping its fields frozen", async () => {
  const retry = vi.fn().mockRejectedValueOnce(new Error("Still unavailable")).mockResolvedValue(undefined);
  const move = vi.fn().mockRejectedValue(new AppliedChangeError(retry));
  const close = vi.fn();
  render(<MoveMoneyPopover budget={demoBudget()} anchor={{ current: document.body }} source="groceries"
    sourceName="Groceries" available={-1000} balances={new Map()} readyToAssign={10000} onMove={move} onClose={close} />);
  fireEvent.click(screen.getByRole("button", { name: "Move" }));
  await screen.findByRole("button", { name: "Retry saving" });
  expect(screen.getByLabelText("Amount").matches(":disabled")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Retry saving" }));
  await screen.findByText("Still unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Retry saving" }));
  await waitFor(() => expect(close).toHaveBeenCalledTimes(1));
  expect(move).toHaveBeenCalledTimes(1);
  expect(retry).toHaveBeenCalledTimes(2);
});
