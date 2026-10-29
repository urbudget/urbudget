import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ManageBudgetsDialog, NewBudgetDialog } from "./BudgetDialogs";
import { api } from "./bridge";
import { demoBudget } from "./demo";
import type { Summary } from "./types";

vi.mock("./bridge", () => ({ api: { createBudget: vi.fn(), listBudgets: vi.fn(), libraryAction: vi.fn() } }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

it("can open another budget without offering a summary-dependent new budget action", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([{ id: "other", name: "Other budget", currency: "USD", path: "/other.sqlite3", current: false, missing: false, lastOpened: null }]);
  const open = vi.fn().mockResolvedValue(undefined);
  const close = vi.fn();
  render(<ManageBudgetsDialog open={open} close={close} />);
  fireEvent.click(await screen.findByRole("button", { name: "Open" }));
  await waitFor(() => expect(open).toHaveBeenCalledWith("other"));
  expect(screen.queryByRole("button", { name: "New budget…" })).toBeNull();
  expect(close).toHaveBeenCalled();
});

it("opens the already-created budget on retry after switching failed", async () => {
  vi.mocked(api.createBudget).mockResolvedValue("created-budget");
  const open = vi.fn().mockRejectedValueOnce(new Error("Save the current budget first")).mockResolvedValue(undefined);
  const close = vi.fn();
  const summary: Summary = { readyToAssign: 0, assigned: 0, activity: 0, available: 0, income: 0, netWorth: 0, accounts: [], categories: [] };
  render(<NewBudgetDialog current={{ budget: demoBudget(), summary, token: "0:1" }} open={open} close={close} />);
  fireEvent.change(screen.getByRole("textbox", { name: "Budget name" }), { target: { value: "New plan" } });
  fireEvent.click(screen.getByRole("button", { name: "Create budget" }));
  await screen.findByText(/Save the current budget first/);
  fireEvent.click(screen.getByRole("button", { name: /Create budget|Open created budget/ }));
  await waitFor(() => expect(close).toHaveBeenCalled());
  expect(api.createBudget).toHaveBeenCalledTimes(1);
  expect(open).toHaveBeenNthCalledWith(1, "created-budget");
  expect(open).toHaveBeenNthCalledWith(2, "created-budget");
});

it("binds a fresh start to the source budget and revision used for its balances", async () => {
  vi.mocked(api.createBudget).mockResolvedValue("fresh-budget");
  const budget = demoBudget();
  const summary: Summary = { readyToAssign: 0, assigned: 0, activity: 0, available: 0, income: 0, netWorth: 0, accounts: [], categories: [] };
  render(<NewBudgetDialog current={{ budget, summary, token: "source:4:1" }} open={vi.fn()} close={vi.fn()} />);
  fireEvent.change(screen.getByRole("textbox", { name: "Budget name" }), { target: { value: "Fresh plan" } });
  fireEvent.click(screen.getByRole("radio", { name: /Start over from/ }));
  fireEvent.click(screen.getByRole("button", { name: "Create budget" }));
  await waitFor(() => expect(api.createBudget).toHaveBeenCalledWith(
    expect.objectContaining({ name: "Fresh plan", currency: budget.currency }),
    { budget, token: "source:4:1" }, false,
  ));
});

it("opens explicit deletion review instead of restoring over unseen remote edits", async () => {
  const entry = {id: "changed", name: "Changed budget", currency: "USD", path: "/changed.sqlite3", current: false, missing: false, lastOpened: null, lifecycle: "trashed", status: "deletion_conflict", scope: "account"} as const;
  vi.mocked(api.listBudgets).mockResolvedValue([entry]);
  vi.mocked(api.libraryAction).mockResolvedValue({library: {entries: [{...entry, lifecycle: "active"}], activeId: "changed"}} as never);
  const changed = vi.fn(async () => {});
  const close = vi.fn();
  render(<ManageBudgetsDialog open={vi.fn()} close={close} changed={changed} />);
  fireEvent.click(await screen.findByRole("button", {name: "Review newer version"}));
  await waitFor(() => expect(api.libraryAction).toHaveBeenCalledWith("review_deletion", "changed"));
  expect(screen.queryByRole("button", {name: "Restore"})).toBeNull();
  await waitFor(() => expect(changed).toHaveBeenCalledOnce());
  expect(close).toHaveBeenCalledOnce();
});