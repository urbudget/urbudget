import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { coverAmount, MoveMoneyPopover } from "./MoneyPopovers";
import { AppliedChangeError } from "./useBudget";
import { demoBudget } from "./demo";
import { editableAmount } from "./format";
import { READY_TO_ASSIGN } from "./ui";
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

it("opens Assign money with the amount empty, not with all of Unassigned for one category", () => {
  const props = { budget: demoBudget(), anchor: { current: document.body }, source: READY_TO_ASSIGN, sourceName: "Unassigned",
    balances: new Map<string, number>(), onMove: vi.fn(), onClose: vi.fn() };
  render(<MoveMoneyPopover {...props} available={370000} readyToAssign={370000} />);
  expect(screen.getByText("Assign money")).toBeTruthy();
  expect((screen.getByLabelText("Amount") as HTMLInputElement).value).toBe("");
  // Nothing is assigned until an amount is typed.
  fireEvent.click(screen.getByRole("button", { name: "Assign" }));
  expect(screen.getByRole("alert").textContent).toBe("Enter an amount above zero.");
  expect(props.onMove).not.toHaveBeenCalled();
  cleanup();
  // More assigned than there is: taking it back still opens with the shortfall.
  render(<MoveMoneyPopover {...props} available={-2500} readyToAssign={-2500} />);
  expect((screen.getByLabelText("Amount") as HTMLInputElement).value).toBe(editableAmount(2500));
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
