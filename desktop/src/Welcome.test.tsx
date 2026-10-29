import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { produce } from "immer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("./bridge", () => ({
  api: {
    listBudgets: vi.fn(async () => []),
    createBudget: vi.fn(async () => "sample-id"),
    deleteBudget: vi.fn(async () => []),
  },
}));
import { api } from "./bridge";
import { FirstRunWelcome, GettingStarted, SampleBanner, Welcome, isBlank, isUntouched, openSample } from "./Welcome";
import { ToastProvider } from "./ui";
import type { Budget } from "./types";

const blank = (): Budget => ({
  schemaVersion: 2,
  name: "My budget",
  currency: "USD",
  numberFormat: { decimal: ".", group: ",", symbol: "before" },
  dateFormat: "mdy",
  accounts: [],
  categories: [],
  transactions: [],
  assignments: [],
});
beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
});
afterEach(cleanup);

it("knows a first launch's budget from one someone set up", () => {
  const { numberFormat: _n, dateFormat: _d, ...fresh } = { ...blank(), currency: "SAR" };
  expect(isUntouched(fresh)).toBe(true);
  expect(isUntouched(blank())).toBe(false);
  expect(isBlank(blank())).toBe(true);
  expect(isBlank({ ...blank(), categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }] })).toBe(false);
});

it("starts fresh with starter categories and the chosen formats, then asks for an account", () => {
  let budget = blank();
  const edit = (update: (d: Budget) => void) => (budget = produce(budget, update));
  const onAddAccount = vi.fn();
  render(
    <ToastProvider>
      <Welcome
        budget={budget}
        budgetId="b1"
        edit={edit}
        transform={vi.fn()}
        openBudget={vi.fn()}
        onAddAccount={onAddAccount}
        importYnab={vi.fn()}
        resolveYnab={vi.fn()} acceptYnab={vi.fn()}
      />
    </ToastProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: /Start fresh/ }));
  fireEvent.click(screen.getByRole("button", { name: /Next: add your accounts/ }));
  expect(budget.categories.map((c) => c.group)).toContain("Savings");
  expect(budget.groupOrder).toEqual(["Bills", "Needs", "Wants", "Savings"]);
  expect(budget.dateFormat).toBe("mdy");
  expect(onAddAccount).toHaveBeenCalled();
  expect(localStorage.getItem("lb:setup:b1")).toBe("on");
});

it("opens the sample as its own budget in this budget's currency", async () => {
  const openBudget = vi.fn(async () => {});
  render(
    <ToastProvider>
      <Welcome
        budget={{ ...blank(), currency: "CAD" }}
        budgetId="b1"
        edit={vi.fn()}
        transform={vi.fn()}
        openBudget={openBudget}
        onAddAccount={vi.fn()}
        importYnab={vi.fn()}
        resolveYnab={vi.fn()} acceptYnab={vi.fn()}
      />
    </ToastProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: /Look around first/ }));
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  const sample = vi.mocked(api.createBudget).mock.calls[0][0];
  expect(sample).toMatchObject({ name: "Sample budget", currency: "CAD", sample: { from: "b1" } });
  expect(sample.accounts.length).toBeGreaterThan(0);
});

it("leaves the sample for the budget it came from, deleting it if asked", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "other", name: "Other", currency: "USD", current: false, missing: false, path: "", lastOpened: null },
    { id: "b1", name: "My budget", currency: "USD", current: false, missing: false, path: "", lastOpened: null },
    { id: "s", name: "Sample budget", currency: "USD", current: true, missing: false, path: "", lastOpened: null },
  ] as never);
  const openBudget = vi.fn(async () => {});
  render(
    <ToastProvider>
      <SampleBanner budget={{ ...blank(), sample: { from: "b1" } }} budgetId="s" openBudget={openBudget} onNewBudget={vi.fn()} />
    </ToastProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete sample" }));
  await vi.waitFor(() => expect(api.deleteBudget).toHaveBeenCalledWith("s"));
  expect(openBudget).toHaveBeenCalledWith("b1");
});

it("walks a budget set up from the welcome screen through the method, step by step", () => {
  const props = { onAddAccount: vi.fn(), onAssign: vi.fn(), onAddTransaction: vi.fn(), onSetTarget: vi.fn() };
  const budget: Budget = {
    ...blank(),
    accounts: [{ id: "a", name: "Checking", kind: "cash", openingBalance: 100000, archived: false }],
    categories: [{ id: "c", name: "Rent", group: "Bills", target: 0 }],
  };
  // Budgets that never saw the welcome screen don't get it.
  const { rerender } = render(<GettingStarted budget={budget} budgetId="b1" {...props} />);
  expect(screen.queryByRole("region", { name: "Getting started" })).toBeNull();

  localStorage.setItem("lb:setup:b2", "on");
  rerender(<GettingStarted budget={budget} budgetId="b2" {...props} />);
  expect(screen.getByText("1 of 5 done")).toBeTruthy();
  expect(screen.getByRole("button", { name: /Assign your money/ }).getAttribute("aria-current")).toBe("step");
  fireEvent.click(screen.getByRole("button", { name: "Assign money" }));
  expect(props.onAssign).toHaveBeenCalled();

  const assigned = { ...budget, assignments: [{ month: "2026-10", categoryId: "c", amount: 50000 }] };
  rerender(<GettingStarted budget={assigned} budgetId="b2" {...props} />);
  expect(screen.getByText("2 of 5 done")).toBeTruthy();
  expect(screen.getByRole("button", { name: /Record your spending/ }).getAttribute("aria-current")).toBe("step");

  // Imported budgets already know the method.
  rerender(<GettingStarted budget={{ ...assigned, importedFrom: "ynabFiles" }} budgetId="b2" {...props} />);
  expect(screen.queryByRole("region", { name: "Getting started" })).toBeNull();

  rerender(<GettingStarted budget={assigned} budgetId="b2" {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Hide getting started" }));
  expect(screen.queryByRole("region", { name: "Getting started" })).toBeNull();
  expect(localStorage.getItem("lb:setup:b2")).toBe("off");
});

it("leads the YNAB choice with the token import, with the export file a click away", () => {
  render(
    <ToastProvider>
      <Welcome
        budget={blank()}
        budgetId="b1"
        edit={vi.fn()}
        transform={vi.fn()}
        openBudget={vi.fn()}
        onAddAccount={vi.fn()}
        importYnab={vi.fn()}
        resolveYnab={vi.fn()} acceptYnab={vi.fn()}
      />
    </ToastProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: /I'm coming from YNAB/ }));
  expect(screen.getByLabelText("YNAB personal access token")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "No token? Use YNAB's export file" }));
  expect(screen.getByRole("button", { name: /Choose export file/ })).toBeTruthy();
  expect(screen.queryByLabelText("YNAB personal access token")).toBeNull();
});

it("creates a new sample when the previous sample is in Trash or recovery", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "trashed-sample", name: "Sample budget", missing: false, lifecycle: "trashed" },
    { id: "recovered-sample", name: "Sample budget", missing: false, lifecycle: "recovery" },
  ] as never);
  const open = vi.fn(async () => {});
  await openSample(blank(), "mine", open);
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(open).toHaveBeenCalledWith("sample-id");
});

it("leaves a sample for an active budget when its original budget is in Trash", async () => {
  vi.mocked(api.listBudgets).mockResolvedValue([
    { id: "old", name: "Old budget", missing: false, lifecycle: "trashed" },
    { id: "mine", name: "My budget", missing: false, lifecycle: "active" },
    { id: "sample", name: "Sample budget", missing: false, lifecycle: "active" },
  ] as never);
  const open = vi.fn(async () => {});
  render(<ToastProvider><SampleBanner budget={{...blank(), sample: {from: "old"}}} budgetId="sample" openBudget={open} onNewBudget={vi.fn()} /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", {name: "Go to my budget"}));
  await vi.waitFor(() => expect(open).toHaveBeenCalledWith("mine"));
});

it("offers first-run choices without creating a placeholder, then creates the selected fresh budget", async () => {
  const openBudget = vi.fn(async () => {}), onAddAccount = vi.fn();
  render(<ToastProvider><FirstRunWelcome openBudget={openBudget} onAddAccount={onAddAccount}
    importYnab={vi.fn()} resolveYnab={vi.fn()} acceptYnab={vi.fn()} /></ToastProvider>);
  expect(screen.getByRole("button", {name: /I'm coming from YNAB/})).toBeTruthy();
  expect(screen.getByRole("button", {name: /Look around first/})).toBeTruthy();
  expect(api.createBudget).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name: /Start fresh/}));
  expect(api.createBudget).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name: /Next: add your accounts/}));
  await vi.waitFor(() => expect(onAddAccount).toHaveBeenCalledOnce());
  expect(api.createBudget).toHaveBeenCalledOnce();
  expect(vi.mocked(api.createBudget).mock.calls[0][0]).toMatchObject({currency:"SAR",groupOrder:["Bills","Needs","Wants","Savings"]});
  expect(openBudget).toHaveBeenCalledWith("sample-id");
});

it("creates an explicit YNAB destination before showing its token form so import review survives", async () => {
  const openBudget = vi.fn(async () => {});
  const {unmount} = render(<ToastProvider><FirstRunWelcome openBudget={openBudget} onAddAccount={vi.fn()}
    importYnab={vi.fn()} resolveYnab={vi.fn()} acceptYnab={vi.fn()} /></ToastProvider>);
  fireEvent.click(screen.getByRole("button", {name:/I'm coming from YNAB/}));
  await vi.waitFor(() => expect(openBudget).toHaveBeenCalledWith("sample-id"));
  unmount();
  render(<ToastProvider><Welcome budget={blank()} budgetId="sample-id" openBudget={openBudget} onAddAccount={vi.fn()}
    edit={vi.fn()} transform={vi.fn()} importYnab={vi.fn()} resolveYnab={vi.fn()} acceptYnab={vi.fn()} /></ToastProvider>);
  expect(screen.getByLabelText("YNAB personal access token")).toBeTruthy();
  expect(sessionStorage.getItem("lb:welcome-ynab:sample-id")).toBeNull();
});
